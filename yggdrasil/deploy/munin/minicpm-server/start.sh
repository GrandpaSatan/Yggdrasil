#!/usr/bin/env bash
set -e

# Use the minicpm-env venv with ROCm PyTorch
VENV=/home/jhernandez/minicpm-env
TORCH_LIB=$($VENV/bin/python3 -c "import torch; import os; print(os.path.join(os.path.dirname(torch.__file__), 'lib'))")
export LD_LIBRARY_PATH="${TORCH_LIB}:${LD_LIBRARY_PATH:-}"
export PYTHONUNBUFFERED=1

exec $VENV/bin/python3 server.py --port 9098
