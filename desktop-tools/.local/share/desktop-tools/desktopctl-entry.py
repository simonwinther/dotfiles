"""Run desktopctl as an imported module, so Python can load its cached bytecode."""
import sys

import desktopctl

# Keep argparse's program name, usage, and errors identical to running desktopctl.py.
sys.argv[0] = desktopctl.__file__
raise SystemExit(desktopctl.main())
