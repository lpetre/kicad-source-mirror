/*
 * This program source code file is part of KiCad, a free EDA CAD application.
 *
 * This program is free software: you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the
 * Free Software Foundation, either version 3 of the License, or (at your
 * option) any later version.
 *
 * This program is distributed in the hope that it will be useful, but
 * WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License along
 * with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

// pcb-dump: helpers shared by the item and defaults dumpers.

#pragma once

#include "pcb_dump.h"

#include <geometry/eda_angle.h>
#include <geometry/geometry_utils.h>
#include <board_item.h>

class EDA_SHAPE;
class PADSTACK;
class BOARD_DESIGN_SETTINGS;
class NETCLASS;
class BOARD_STACKUP;
class PCB_PLOT_PARAMS;
class TEARDROP_PARAMETERS;
class ZONE_SETTINGS;

/// One (clearance, maxError, errorLoc) combination for TransformShapeToPolygon dumps.
struct POLY_COMBO
{
    int         clearance;
    int         maxError;
    ERROR_LOC   errorLoc;
    std::string name;
};

const std::vector<POLY_COMBO>& PolyCombos();

void DumpTransformedPolys( BOARD_ITEM* aItem, PCB_LAYER_ID aLayer );
void DumpEffectiveShape( const std::string& aField, BOARD_ITEM* aItem, PCB_LAYER_ID aLayer,
                         FLASHING aFlash );
void DumpPadstack( const std::string& aPrefix, const PADSTACK& aPadstack );
void DumpEdaShape( const std::string& aPrefix, EDA_SHAPE* aShape );

void DumpDesignSettings( const std::string& aPrefix, const BOARD_DESIGN_SETTINGS& aBds );
void DumpNetclass( const std::string& aPrefix, const NETCLASS& aNc );
void DumpStackup( const std::string& aPrefix, const BOARD_STACKUP& aStackup );
void DumpPlotParams( const std::string& aPrefix, const PCB_PLOT_PARAMS& aParams );
void DumpTeardropParams( const std::string& aPrefix, const TEARDROP_PARAMETERS& aParams );
void DumpZoneSettings( const std::string& aPrefix, const ZONE_SETTINGS& aSettings );
