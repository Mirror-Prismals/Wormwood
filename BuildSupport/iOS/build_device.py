#!/usr/bin/env python3
"""Build the native game for a registered iPhone. Tests never run implicitly."""
import sys
from build_ios import main
if __name__=="__main__":
 if "--build-only" in sys.argv:sys.argv.remove("--build-only")
 else:sys.argv.extend(["--install","--launch"])
 main()
