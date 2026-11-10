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

// pcb-dump: the PROPERTY_MANAGER tables ("props-table" command) and the per-item property
// values ("props" section, opt-in), see rust/docs/PCB_DUMP_FORMAT.md.

#include "pcb_dump.h"

#include <properties/property.h>
#include <properties/property_mgr.h>
#include <pcbexpr_evaluator.h>

#include <board.h>
#include <board_connected_item.h>
#include <board_item_container.h>
#include <eda_item.h>
#include <eda_shape.h>
#include <eda_text.h>
#include <footprint.h>
#include <line_ending.h>
#include <marker_base.h>
#include <pad.h>
#include <padstack.h>
#include <pcb_3d_body.h>
#include <pcb_barcode.h>
#include <pcb_dimension.h>
#include <pcb_drill_chart.h>
#include <pcb_drill_map.h>
#include <pcb_field.h>
#include <pcb_generated_table.h>
#include <pcb_generator.h>
#include <pcb_grid_item.h>
#include <pcb_group.h>
#include <pcb_marker.h>
#include <pcb_point.h>
#include <pcb_reference_image.h>
#include <pcb_shape.h>
#include <pcb_stackup_table.h>
#include <pcb_table.h>
#include <pcb_tablecell.h>
#include <pcb_target.h>
#include <pcb_text.h>
#include <pcb_textbox.h>
#include <pcb_track.h>
#include <zone.h>
#include <zone_settings.h>
#include <constraints/pcb_constraint.h>
#include <generators/pcb_tuning_pattern.h>
#include <generators/pcb_via_stack.h>
#include <generators/pcb_via_stitch.h>
#include <board_tables/generated_table_schema.h>
#include <router/pns_meander.h>
#include <gal/color4d.h>

#include <algorithm>
#include <map>
#include <vector>


using KIGFX::COLOR4D;

namespace
{

struct NAMED_TYPE
{
    size_t      hash;
    const char* name;
};

#define CLS( x ) { TYPE_HASH( x ), #x }

/// Every class the property tables know (registered or only referenced as a base).
const std::vector<NAMED_TYPE>& knownClasses()
{
    static const std::vector<NAMED_TYPE> list = {
        CLS( EDA_ITEM ), CLS( BOARD_ITEM ), CLS( BOARD_CONNECTED_ITEM ), CLS( BOARD_ITEM_CONTAINER ),
        CLS( EDA_SHAPE ), CLS( EDA_TEXT ), CLS( EDA_GROUP ), CLS( MARKER_BASE ), CLS( PAD ),
        CLS( PCB_TRACK ), CLS( PCB_ARC ), CLS( PCB_VIA ), CLS( ZONE ), CLS( FOOTPRINT ),
        CLS( PCB_SHAPE ), CLS( PCB_TEXT ), CLS( PCB_TEXTBOX ), CLS( PCB_FIELD ),
        CLS( PCB_DIMENSION_BASE ), CLS( PCB_DIM_ALIGNED ), CLS( PCB_DIM_ORTHOGONAL ),
        CLS( PCB_DIM_RADIAL ), CLS( PCB_DIM_LEADER ), CLS( PCB_DIM_CENTER ), CLS( PCB_GROUP ),
        CLS( PCB_GENERATOR ), CLS( PCB_TABLE ), CLS( PCB_TABLECELL ), CLS( PCB_BARCODE ),
        CLS( PCB_REFERENCE_IMAGE ), CLS( PCB_3D_BODY ), CLS( PCB_DRILL_CHART ), CLS( PCB_DRILL_MAP ),
        CLS( PCB_GENERATED_TABLE ), CLS( PCB_GRID_ITEM ), CLS( PCB_MARKER ), CLS( PCB_POINT ),
        CLS( PCB_STACKUP_TABLE ), CLS( PCB_TARGET ), CLS( PCB_CONSTRAINT ), CLS( PCB_TUNING_PATTERN ),
        CLS( PCB_VIA_STACK ), CLS( PCB_VIA_STITCH ),
    };

    return list;
}


/// Value types of the registered properties.
const std::vector<NAMED_TYPE>& knownValueTypes()
{
    static const std::vector<NAMED_TYPE> list = {
        CLS( int ), CLS( unsigned ), CLS( long long ), CLS( double ), CLS( bool ), CLS( wxString ),
        { TYPE_HASH( std::optional<int> ), "std::optional<int>" },
        { TYPE_HASH( std::optional<double> ), "std::optional<double>" },
        CLS( EDA_ANGLE ), CLS( COLOR4D ), CLS( KICAD_T ), CLS( PCB_LAYER_ID ), CLS( SHAPE_T ),
        CLS( LINE_STYLE ), CLS( UI_FILL_MODE ), CLS( LINE_ENDING_STYLE ), CLS( GR_TEXT_H_ALIGN_T ),
        CLS( GR_TEXT_V_ALIGN_T ), CLS( PAD_ATTRIB ), CLS( PAD_SHAPE ), CLS( PAD_PROP ),
        CLS( PAD_DRILL_SHAPE ), CLS( PAD_SIM_ELECTRICAL_TYPE ), CLS( PAD_DRILL_POST_MACHINING_MODE ),
        CLS( BACKDRILL_MODE ), CLS( ZONE_CONNECTION ), CLS( UNCONNECTED_LAYER_MODE ), CLS( VIATYPE ),
        CLS( TENTING_MODE ), CLS( COVERING_MODE ), CLS( PLUGGING_MODE ), CLS( CAPPING_MODE ),
        CLS( FILLING_MODE ), CLS( ZONE_FILL_MODE ), CLS( THIEVING_PATTERN ), CLS( ISLAND_REMOVAL_MODE ),
        CLS( PLACEMENT_SOURCE_T ), CLS( FOOTPRINT_TYPE ), CLS( DIM_PRECISION ), CLS( DIM_UNITS_FORMAT ),
        CLS( DIM_UNITS_MODE ), CLS( DIM_ARROW_DIRECTION ), CLS( DIM_VALUE_MODE ),
        CLS( DIM_TEXT_POSITION ), CLS( DIM_TEXT_BORDER ), CLS( BARCODE_T ), CLS( BARCODE_ECC_T ),
        CLS( GENERATED_TABLE_UNITS ), CLS( PCB_GRID_TYPE ), CLS( LENGTH_TUNING_MODE ),
        { TYPE_HASH( PNS::MEANDER_SIDE ), "PNS::MEANDER_SIDE" }, CLS( VIA_STACK_STYLE ),
        CLS( PCB_VIA_STITCH_LAYOUT ), CLS( PCB_VIA_STITCH_MODE ),
    };

    return list;
}

#undef CLS


std::string typeName( const std::vector<NAMED_TYPE>& aList, size_t aHash )
{
    for( const NAMED_TYPE& t : aList )
    {
        if( t.hash == aHash )
            return t.name;
    }

    return "?";
}


std::string className( size_t aHash ) { return typeName( knownClasses(), aHash ); }


std::string vChoices( const wxPGChoices& aChoices )
{
    std::string r = "[";

    for( unsigned ii = 0; ii < aChoices.GetCount(); ++ii )
    {
        if( ii )
            r += ",";

        r += std::to_string( aChoices.GetValue( ii ) ) + ":" + V( aChoices.GetLabel( ii ) );
    }

    return r + "]";
}


template <typename T>
void dumpEnumMap( const char* aName )
{
    ENUM_MAP<T>& map = ENUM_MAP<T>::Instance();
    SCOPE        s( "enummap", aName );
    EMIT( "undefined", std::to_string( static_cast<long long>( map.GetUndefined() ) ) );
    EMIT( "choices", vChoices( map.Choices() ) );
}


/// The properties of a class in display order (GetDisplayOrder), each once.
std::vector<std::pair<int, PROPERTY_BASE*>> displayOrdered( size_t aType )
{
    std::vector<std::pair<int, PROPERTY_BASE*>> r;

    for( const auto& [prop, order] : PROPERTY_MANAGER::Instance().GetDisplayOrder( aType ) )
        r.emplace_back( order, prop );

    std::sort( r.begin(), r.end(),
               []( const auto& a, const auto& b )
               {
                   return a.first < b.first;
               } );

    return r;
}


std::string vBool01( bool aVal ) { return aVal ? "1" : "0"; }

} // namespace


/// The "props-table" command: every class, its ancestors, groups and properties, and every
/// ENUM_MAP used by a property.
void DumpPropsTable()
{
    PROPERTY_MANAGER& pm = PROPERTY_MANAGER::Instance();
    pm.Rebuild();

    std::vector<PROPERTY_MANAGER::CLASS_INFO> classes = pm.GetAllClasses();
    std::sort( classes.begin(), classes.end(),
               []( const auto& a, const auto& b )
               {
                   return className( a.type ) < className( b.type );
               } );

    for( const PROPERTY_MANAGER::CLASS_INFO& cls : classes )
    {
        const std::string name = className( cls.type );
        SCOPE             s( "propclass", name );

        EMIT( "registered", V( cls.name ) );

        std::vector<std::string> ancestors;

        for( const NAMED_TYPE& other : knownClasses() )
        {
            if( other.hash != cls.type && pm.IsOfType( cls.type, other.hash ) )
                ancestors.emplace_back( other.name );
        }

        std::sort( ancestors.begin(), ancestors.end() );
        std::string a = "[";

        for( size_t i = 0; i < ancestors.size(); ++i )
            a += ( i ? "," : "" ) + ancestors[i];

        EMIT( "ancestors", a + "]" );

        std::string g = "[";
        const std::vector<wxString>& groups = pm.GetGroupDisplayOrder( cls.type );

        for( size_t i = 0; i < groups.size(); ++i )
            g += ( i ? "," : "" ) + V( groups[i] );

        EMIT( "groups", g + "]" );
        EMIT( "count", V( (int) pm.GetProperties( cls.type ).size() ) );

        std::vector<std::pair<int, PROPERTY_BASE*>> props = displayOrdered( cls.type );
        EMIT( "distinct", V( (int) props.size() ) );

        int idx = 0;

        for( const auto& [order, p] : props )
        {
            const std::string field = std::to_string( idx++ );
            std::string       v;

            v += "name=" + V( p->Name() );
            v += ";order=" + std::to_string( order );
            v += ";owner=" + className( p->OwnerHash() );
            v += ";base=" + className( p->BaseHash() );
            v += ";type=" + typeName( knownValueTypes(), p->TypeHash() );
            v += ";display=" + VEnum( p->Display() );
            v += ";coord=" + VEnum( p->CoordType() );
            v += ";group=" + V( p->Group() );
            v += ";has_choices=" + vBool01( p->HasChoices() );
            v += ";choices=" + vChoices( p->Choices() );
            v += ";hidden_pm=" + vBool01( p->IsHiddenFromPropertiesManager() );
            v += ";hidden_re=" + vBool01( p->IsHiddenFromRulesEditor() );
            v += ";hidden_lib=" + vBool01( p->IsHiddenFromLibraryEditors() );
            v += ";hidden_des=" + vBool01( p->IsHiddenFromDesignEditors() );
            v += ";copyable=" + vBool01( p->IsCopyable() );
            v += ";kind=" + VEnum( PCBEXPR_VAR_REF::ClassifyProperty( p ) );
            v += ";lookup=" + vBool01( pm.GetProperty( cls.type, p->Name() ) == p );

            EMIT( field, v );
        }
    }

    dumpEnumMap<KICAD_T>( "KICAD_T" );
    dumpEnumMap<PCB_LAYER_ID>( "PCB_LAYER_ID" );
    dumpEnumMap<SHAPE_T>( "SHAPE_T" );
    dumpEnumMap<LINE_STYLE>( "LINE_STYLE" );
    dumpEnumMap<UI_FILL_MODE>( "UI_FILL_MODE" );
    dumpEnumMap<LINE_ENDING_STYLE>( "LINE_ENDING_STYLE" );
    dumpEnumMap<GR_TEXT_H_ALIGN_T>( "GR_TEXT_H_ALIGN_T" );
    dumpEnumMap<GR_TEXT_V_ALIGN_T>( "GR_TEXT_V_ALIGN_T" );
    dumpEnumMap<PAD_ATTRIB>( "PAD_ATTRIB" );
    dumpEnumMap<PAD_SHAPE>( "PAD_SHAPE" );
    dumpEnumMap<PAD_PROP>( "PAD_PROP" );
    dumpEnumMap<PAD_DRILL_SHAPE>( "PAD_DRILL_SHAPE" );
    dumpEnumMap<PAD_SIM_ELECTRICAL_TYPE>( "PAD_SIM_ELECTRICAL_TYPE" );
    dumpEnumMap<PAD_DRILL_POST_MACHINING_MODE>( "PAD_DRILL_POST_MACHINING_MODE" );
    dumpEnumMap<BACKDRILL_MODE>( "BACKDRILL_MODE" );
    dumpEnumMap<ZONE_CONNECTION>( "ZONE_CONNECTION" );
    dumpEnumMap<UNCONNECTED_LAYER_MODE>( "UNCONNECTED_LAYER_MODE" );
    dumpEnumMap<VIATYPE>( "VIATYPE" );
    dumpEnumMap<TENTING_MODE>( "TENTING_MODE" );
    dumpEnumMap<COVERING_MODE>( "COVERING_MODE" );
    dumpEnumMap<PLUGGING_MODE>( "PLUGGING_MODE" );
    dumpEnumMap<CAPPING_MODE>( "CAPPING_MODE" );
    dumpEnumMap<FILLING_MODE>( "FILLING_MODE" );
    dumpEnumMap<ZONE_FILL_MODE>( "ZONE_FILL_MODE" );
    dumpEnumMap<THIEVING_PATTERN>( "THIEVING_PATTERN" );
    dumpEnumMap<ISLAND_REMOVAL_MODE>( "ISLAND_REMOVAL_MODE" );
    dumpEnumMap<PLACEMENT_SOURCE_T>( "PLACEMENT_SOURCE_T" );
    dumpEnumMap<FOOTPRINT_TYPE>( "FOOTPRINT_TYPE" );
    dumpEnumMap<DIM_PRECISION>( "DIM_PRECISION" );
    dumpEnumMap<DIM_UNITS_FORMAT>( "DIM_UNITS_FORMAT" );
    dumpEnumMap<DIM_UNITS_MODE>( "DIM_UNITS_MODE" );
    dumpEnumMap<DIM_ARROW_DIRECTION>( "DIM_ARROW_DIRECTION" );
    dumpEnumMap<DIM_VALUE_MODE>( "DIM_VALUE_MODE" );
    dumpEnumMap<DIM_TEXT_POSITION>( "DIM_TEXT_POSITION" );
    dumpEnumMap<DIM_TEXT_BORDER>( "DIM_TEXT_BORDER" );
    dumpEnumMap<BARCODE_T>( "BARCODE_T" );
    dumpEnumMap<BARCODE_ECC_T>( "BARCODE_ECC_T" );
    dumpEnumMap<GENERATED_TABLE_UNITS>( "GENERATED_TABLE_UNITS" );
    dumpEnumMap<PCB_GRID_TYPE>( "PCB_GRID_TYPE" );
    dumpEnumMap<LENGTH_TUNING_MODE>( "LENGTH_TUNING_MODE" );
    dumpEnumMap<PNS::MEANDER_SIDE>( "PNS::MEANDER_SIDE" );
    dumpEnumMap<VIA_STACK_STYLE>( "VIA_STACK_STYLE" );
    dumpEnumMap<PCB_VIA_STITCH_LAYOUT>( "PCB_VIA_STITCH_LAYOUT" );
    dumpEnumMap<PCB_VIA_STITCH_MODE>( "PCB_VIA_STITCH_MODE" );
}


namespace
{

/// The value of one property the way PCBEXPR_VAR_REF::GetValue computes it, rendered as
/// "n:<double>", "s:<quoted string>", "layer:<name>", "null" or "undef" (the duck-typed number
/// conversion of strings is left out: strings are shown raw).
std::string propValue( BOARD_ITEM* aItem, PROPERTY_BASE* aProp )
{
    switch( PCBEXPR_VAR_REF::ClassifyProperty( aProp ) )
    {
    case PCBEXPR_PROPERTY_KIND::INT_KIND:
        return "n:" + V( static_cast<double>( aItem->Get<int>( aProp ) ) );

    case PCBEXPR_PROPERTY_KIND::OPTIONAL_INT:
    {
        std::optional<int> val = aItem->Get<std::optional<int>>( aProp );
        return val ? "n:" + V( static_cast<double>( *val ) ) : std::string( "null" );
    }

    case PCBEXPR_PROPERTY_KIND::UNSIGNED:
        return "n:" + V( static_cast<double>( aItem->Get<unsigned>( aProp ) ) );

    case PCBEXPR_PROPERTY_KIND::LONG_LONG:
        return "n:" + V( static_cast<double>( aItem->Get<long long int>( aProp ) ) );

    case PCBEXPR_PROPERTY_KIND::DOUBLE: return "n:" + V( aItem->Get<double>( aProp ) );

    case PCBEXPR_PROPERTY_KIND::OPTIONAL_DOUBLE:
    {
        std::optional<double> val = aItem->Get<std::optional<double>>( aProp );
        return val ? "n:" + V( *val ) : std::string( "null" );
    }

    case PCBEXPR_PROPERTY_KIND::BOOL_KIND:
        return "n:" + V( static_cast<double>( aItem->Get<bool>( aProp ) ) );

    case PCBEXPR_PROPERTY_KIND::STRING: return "s:" + V( aItem->Get<wxString>( aProp ) );

    case PCBEXPR_PROPERTY_KIND::ENUM:
    {
        wxString     str;
        const wxAny& any = aItem->Get( aProp );

        if( aProp->Name() == wxT( "Layer" ) || aProp->Name() == wxT( "Layer Top" )
            || aProp->Name() == wxT( "Layer Bottom" ) )
        {
            PCB_LAYER_ID layer;

            if( any.GetAs<PCB_LAYER_ID>( &layer ) )
                return "layer:" + VLayer( layer );
            else if( any.GetAs<wxString>( &str ) )
                return "layername:" + V( str );
        }
        else if( any.GetAs<wxString>( &str ) )
        {
            return "s:" + V( str );
        }

        return "undef";
    }

    case PCBEXPR_PROPERTY_KIND::ANGLE: return "n:" + V( aItem->Get<EDA_ANGLE>( aProp ).AsDegrees() );

    case PCBEXPR_PROPERTY_KIND::COLOR: return "s:" + V( aItem->Get<COLOR4D>( aProp ).ToCSSString() );

    case PCBEXPR_PROPERTY_KIND::UNSUPPORTED: return "undef";
    }

    return "undef";
}

} // namespace


/// The "props" section: every property of the item's class in display order: "n/a" when it is
/// not available for the item, else its value; " (ro)" is appended when it is not writeable.
void DumpProps( BOARD_ITEM* aItem )
{
    PROPERTY_MANAGER& pm = PROPERTY_MANAGER::Instance();
    const size_t      type = TYPE_HASH( *aItem );

    EMIT( "class", className( type ) );

    for( const auto& [order, p] : displayOrdered( type ) )
    {
        const std::string field = std::string( p->Name().ToUTF8() );

        EMIT( field,
              [&]() -> std::string
              {
                  if( !pm.IsAvailableFor( type, p, aItem ) )
                      return "n/a";

                  std::string v = propValue( aItem, p );

                  if( !pm.IsWriteableFor( type, p, aItem ) )
                      v += " (ro)";

                  return v;
              }() );
    }
}
