#!/usr/bin/env python3
"""withapp.py BIN PACK LOG CMD...: run CMD (e.g. ../lagmeter.py) against a test instance of her in the rig."""
import subprocess, sys
from rigapp import RigApp

with RigApp(sys.argv[1], sys.argv[2], sys.argv[3]):
    rc = subprocess.run(sys.argv[4:]).returncode
sys.exit(rc)
