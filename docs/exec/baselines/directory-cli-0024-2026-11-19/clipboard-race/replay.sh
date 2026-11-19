#!/bin/sh
set -eu
# Run from the repository with the source digest recorded in receipt.json.
python3 tools/directorye2e/run.py --output "$1"
