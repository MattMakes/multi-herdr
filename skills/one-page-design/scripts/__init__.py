"""Package marker, so unittest discovery reaches scripts/tests/.

The skill bundle ships every file in the skill folder, so a test run must
leave no bytecode here. Discovery imports this file first. Python writes the
cache for this file before it runs it, so this file turns off bytecode writes
for every later import and then removes its own cache file.
"""

import os
import sys

sys.dont_write_bytecode = True

if __cached__ and os.path.isfile(__cached__):
    os.remove(__cached__)
    try:
        os.rmdir(os.path.dirname(__cached__))
    except OSError:
        pass  # the folder holds other files; leave it
