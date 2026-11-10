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
#
# REV-FAB review harness (rust/docs/design/review-fab.md). Scratch output goes to
# rust/target/rev-fab/work; the reference kicad-cli always runs on copies.
import uuid, itertools
_n = itertools.count(1)
def uid():
    return "%08x-0000-4000-8000-%012x" % (0xabc00000, next(_n))
LAYERS4 = """(layers
 (0 "F.Cu" signal) (1 "In1.Cu" signal) (2 "In2.Cu" signal) (31 "B.Cu" signal)
 (32 "B.Adhes" user "B.Adhesive") (33 "F.Adhes" user "F.Adhesive") (34 "B.Paste" user) (35 "F.Paste" user)
 (36 "B.SilkS" user "B.Silkscreen") (37 "F.SilkS" user "F.Silkscreen") (38 "B.Mask" user) (39 "F.Mask" user)
 (40 "Dwgs.User" user "User.Drawings") (41 "Cmts.User" user "User.Comments") (42 "Eco1.User" user "User.Eco1")
 (43 "Eco2.User" user "User.Eco2") (44 "Edge.Cuts" user) (45 "Margin" user) (46 "B.CrtYd" user "B.Courtyard")
 (47 "F.CrtYd" user "F.Courtyard") (48 "B.Fab" user) (49 "F.Fab" user))"""
LAYERS2 = LAYERS4.replace('(1 "In1.Cu" signal) (2 "In2.Cu" signal) ', '')
def header(layers=LAYERS4, nets=("N1", "N2", "GND"), setup_extra=""):
    s = '(kicad_pcb (version 20240108) (generator "pcbnew") (generator_version "8.0")\n'
    s += ' (general (thickness 1.6) (legacy_teardrops no))\n (paper "A4")\n ' + layers + "\n"
    s += ' (setup (pad_to_mask_clearance 0.05) (solder_mask_min_width 0) %s (aux_axis_origin 100 150) (grid_origin 100 150))\n' % setup_extra
    s += ' (net 0 "")\n'
    for i, n in enumerate(nets):
        s += ' (net %d "%s")\n' % (i + 1, n)
    return s
NETS = {"": 0, "N1": 1, "N2": 2, "GND": 3}
def net(n):
    return '(net %d "%s")' % (NETS[n], n) if n else ""
def edge(x0, y0, x1, y1):
    return ' (gr_rect (start %g %g) (end %g %g) (stroke (width 0.1) (type default)) (fill none) (layer "Edge.Cuts") (uuid "%s"))\n' % (x0, y0, x1, y1, uid())
