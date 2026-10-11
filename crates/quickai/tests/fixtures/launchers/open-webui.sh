#!/bin/sh
# Written by QuickAI: runs the application with the Python environment installed beside it.
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
# the person's own Python variables are not the application's
unset PYTHONPATH PYTHONHOME PYTHONSTARTUP PYTHONUSERBASE VIRTUAL_ENV
PYTHONNOUSERSITE=1
export PYTHONNOUSERSITE
exec "$here/venv/bin/python" -c "import sys; from open_webui import app; sys.argv[0] = 'open-webui'; sys.exit(app())" "$@"
