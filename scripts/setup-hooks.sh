#!/bin/sh
# Setup pre-commit hooks for automatic formatting and linting

set -e

echo "Setting up pre-commit hooks..."

# Check if pre-commit is installed
if ! command -v pre-commit >/dev/null 2>&1; then
    echo "pre-commit not found. Installing..."

    if command -v brew >/dev/null 2>&1; then
        echo "Installing via Homebrew..."
        brew install pre-commit
    elif command -v pip3 >/dev/null 2>&1; then
        echo "Installing via pip3..."
        pip3 install pre-commit
    elif command -v pip >/dev/null 2>&1; then
        echo "Installing via pip..."
        pip install pre-commit
    else
        echo "ERROR: Could not find brew, pip3, or pip to install pre-commit"
        echo "Please install pre-commit manually: https://pre-commit.com/#install"
        exit 1
    fi
fi

# Install the hooks
pre-commit install

echo ""
echo "Pre-commit hooks installed successfully!"
echo ""
echo "The following checks will run before each commit:"
echo "  - cargo fmt (Rust formatting)"
echo "  - cargo clippy (Rust linting)"
echo "  - shellcheck (shell script linting)"
echo "  - shfmt (shell script formatting)"
echo "  - trailing whitespace removal"
echo "  - end-of-file fixer"
echo ""
echo "To run manually: pre-commit run --all-files"
echo "To skip hooks: git commit --no-verify"
