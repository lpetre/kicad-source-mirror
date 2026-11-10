# This program source code file is part of KiCad, a free EDA CAD application.
#
# Copyright The KiCad Developers, see AUTHORS.txt for contributors.
#
# This program is free software: you can redistribute it and/or modify it
# under the terms of the GNU General Public License as published by the
# Free Software Foundation, either version 3 of the License, or (at your
# option) any later version.
#
# This program is distributed in the hope that it will be useful, but
# WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
# General Public License for more details.
#
# You should have received a copy of the GNU General Public License along
# with this program.  If not, see <http://www.gnu.org/licenses/>.
"""Reduce a .kicad_pcb to its footprints for the PCBIO-07 oracle (pcbio07_golden.sh).

Usage: pcbio07_strip.py IN OUT [--max-footprints N]

pcbio06_strip.py with the pads kept (groups and constraints are still dropped).
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import pcbio06_strip  # noqa: E402

pcbio06_strip.FP_DROP = {b'group', b'constraint'}

if __name__ == '__main__':
    pcbio06_strip.main()
