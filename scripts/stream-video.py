#!/usr/bin/env python3
"""Stream video to LED matrix over WebSocket.

Decodes video on the local machine using ffmpeg and sends raw RGB frames
to the Pi's WebSocket endpoint at the video's native fps.

Requirements:
    pip install websockets rich typer
    ffmpeg and ffprobe must be on PATH
"""

import asyncio
import json
import signal
import subprocess
import threading
import time
from pathlib import Path
from queue import Empty, Queue

import typer
import websockets
from rich.console import Console
from rich.live import Live
from rich.table import Table
from typing import Annotated

app = typer.Typer(help="Stream video to an LED matrix over WebSocket.")
console = Console()

# Sentinel value to signal end-of-stream from the reader thread
_EOF = object()


def get_video_info(video_path: str) -> tuple[float, float]:
    """Extract video fps and duration using ffprobe."""
    result = subprocess.run(
        [
            "ffprobe",
            "-v", "quiet",
            "-print_format", "json",
            "-show_streams",
            "-show_format",
            "-select_streams", "v:0",
            video_path,
        ],
        capture_output=True,
        text=True,
    )
    info = json.loads(result.stdout)
    stream = info["streams"][0]

    rate_str = stream["r_frame_rate"]  # e.g. "30/1"
    num, den = rate_str.split("/")
    fps = float(num) / float(den)

    duration = float(info.get("format", {}).get("duration", 0))

    return fps, duration


def frame_reader(proc: subprocess.Popen, queue: Queue, frame_size: int, stop: threading.Event) -> None:
    """Background thread: read frames from ffmpeg stdout into the queue."""
    try:
        while not stop.is_set():
            data = proc.stdout.read(frame_size)
            if len(data) < frame_size:
                break
            queue.put(data)
    finally:
        queue.put(_EOF)


def make_status_table(
    video_path: str,
    ws_url: str,
    fps: float,
    duration: float,
    state: str,
    frame_count: int,
    elapsed: float,
    actual_fps: float,
    buffer_fill: int,
    buffer_cap: int,
) -> Table:
    """Build a Rich table showing current streaming status."""
    table = Table(show_header=False, box=None, padding=(0, 1))
    table.add_column(style="bold cyan", min_width=10)
    table.add_column()

    state_colors = {
        "Streaming": "bold green",
        "Buffering": "bold blue",
        "Done": "bold green",
    }
    style = state_colors.get(state, "bold yellow")
    table.add_row("Source", video_path)
    table.add_row("Target", ws_url)
    table.add_row("State", f"[{style}]{state}[/]")
    table.add_row("FPS", f"{actual_fps:.1f} / {fps:.1f} target")
    table.add_row("Frames", str(frame_count))
    table.add_row("Buffer", f"{buffer_fill} / {buffer_cap}")

    if duration > 0:
        total_frames = int(duration * fps)
        progress = min(frame_count / total_frames, 1.0) if total_frames > 0 else 0
        bar_width = 30
        filled = int(bar_width * progress)
        bar = f"[green]{'━' * filled}[/][dim]{'━' * (bar_width - filled)}[/]"
        table.add_row("Progress", f"{bar} {progress:.0%}")

    elapsed_str = f"{int(elapsed // 60)}:{int(elapsed % 60):02d}"
    if duration > 0:
        remaining = max(0, duration - elapsed)
        remaining_str = f"{int(remaining // 60)}:{int(remaining % 60):02d}"
        table.add_row("Time", f"{elapsed_str} / {remaining_str} remaining")
    else:
        table.add_row("Time", elapsed_str)

    return table


async def stream(
    video_path: str,
    ws_url: str,
    size: int,
    fps_override: float | None,
    buffer_frames: int,
    loop: bool,
) -> None:
    frame_size = size * size * 3
    fps, duration = get_video_info(video_path)
    if fps_override is not None:
        fps = fps_override
    frame_interval = 1.0 / fps

    cancelled = asyncio.Event()
    loop_ref = asyncio.get_event_loop()
    loop_ref.add_signal_handler(signal.SIGINT, cancelled.set)

    frame_count = 0
    start_time = time.monotonic()
    actual_fps = 0.0

    with Live(
        make_status_table(video_path, ws_url, fps, duration, "Connecting...", 0, 0, 0, 0, buffer_frames),
        console=console,
        refresh_per_second=4,
    ) as live:
        try:
            async with websockets.connect(ws_url, max_size=frame_size + 1024) as ws:
                while not cancelled.is_set():
                    # Start ffmpeg and reader thread for this pass
                    ffmpeg = subprocess.Popen(
                        [
                            "ffmpeg",
                            "-i", video_path,
                            "-vf", f"scale={size}:{size}",
                            "-pix_fmt", "rgb24",
                            "-f", "rawvideo",
                            "-v", "quiet",
                            "-",
                        ],
                        stdout=subprocess.PIPE,
                    )

                    queue: Queue = Queue(maxsize=buffer_frames)
                    stop_reader = threading.Event()
                    reader = threading.Thread(
                        target=frame_reader,
                        args=(ffmpeg, queue, frame_size, stop_reader),
                        daemon=True,
                    )
                    reader.start()

                    # Wait for buffer to fill before starting playback
                    while queue.qsize() < min(buffer_frames, buffer_frames) and not cancelled.is_set():
                        live.update(make_status_table(
                            video_path, ws_url, fps, duration,
                            "Buffering", frame_count, time.monotonic() - start_time, actual_fps,
                            queue.qsize(), buffer_frames,
                        ))
                        await asyncio.sleep(0.05)

                    # Send loop
                    eof = False
                    while not cancelled.is_set() and not eof:
                        try:
                            data = queue.get_nowait()
                        except Empty:
                            await asyncio.sleep(0.001)
                            continue

                        if data is _EOF:
                            eof = True
                            break

                        send_time = time.monotonic()
                        await ws.send(data)
                        frame_count += 1

                        elapsed = send_time - start_time
                        if elapsed > 0:
                            actual_fps = frame_count / elapsed

                        live.update(make_status_table(
                            video_path, ws_url, fps, duration,
                            "Streaming", frame_count, elapsed, actual_fps,
                            queue.qsize(), buffer_frames,
                        ))

                        # Pace to target fps
                        elapsed_frame = time.monotonic() - send_time
                        sleep_time = frame_interval - elapsed_frame
                        if sleep_time > 0:
                            await asyncio.sleep(sleep_time)

                    # Clean up this pass
                    stop_reader.set()
                    ffmpeg.terminate()
                    ffmpeg.wait()
                    reader.join(timeout=2)

                    if not loop or cancelled.is_set():
                        break

        except ConnectionRefusedError:
            live.update(make_status_table(
                video_path, ws_url, fps, duration,
                "Connection refused", frame_count, time.monotonic() - start_time, actual_fps,
                0, buffer_frames,
            ))
            console.print(f"\n[bold red]Could not connect to {ws_url}[/]")
            return
        except websockets.ConnectionClosed as e:
            console.print(f"\n[bold red]Connection closed: {e}[/]")
            return

    elapsed = time.monotonic() - start_time
    if elapsed > 0:
        console.print(f"\n[bold]Done[/] — {frame_count} frames in {elapsed:.1f}s ({frame_count / elapsed:.1f} fps)")
    else:
        console.print(f"\n[bold]Done[/] — {frame_count} frames")


@app.command()
def main(
    video: Annotated[Path, typer.Argument(help="Path to video file")],
    url: Annotated[str, typer.Argument(help="WebSocket URL, e.g. ws://pi:8080/api/v1/display/stream")],
    size: Annotated[int, typer.Option(help="Panel dimension (pixels per side)")] = 64,
    fps: Annotated[float | None, typer.Option(help="Override video fps")] = None,
    buffer: Annotated[int, typer.Option(help="Number of frames to buffer ahead")] = 30,
    loop: Annotated[bool, typer.Option("--loop", help="Loop video playback")] = False,
) -> None:
    """Stream video to an LED matrix over WebSocket."""
    if not video.exists():
        console.print(f"[bold red]File not found: {video}[/]")
        raise typer.Exit(1)

    asyncio.run(stream(str(video), url, size, fps, buffer, loop))


if __name__ == "__main__":
    app()
