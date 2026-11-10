/*
 * This program source code file is part of KiCad, a free EDA CAD application.
 *
 * Copyright The KiCad Developers, see AUTHORS.txt for contributors.
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

/*
 * Differential oracle for kicad-fab FAB-02 (EXCELLON_WRITER, GenDrillReportFile and the
 * Excellon half of PCBNEW_JOBS_HANDLER::JobExportDrill).
 *
 * Linked against the real pcbnew objects of the reference build (build_excellon_oracle.sh).
 * The clock is pinned: this file defines time() (wxDateTime::Now / GetTimeNow call it), and
 * TZ is set to a +05:30 zone, so that every header is reproducible.
 *
 * Output (fields separated by '|'), see tests/excellon_oracle.rs:
 *
 *   version|<GetBuildVersion()>
 *   val|<metric>|<i>|<x %.17g>|<y %.17g>                                   coordinate table
 *   coord|<metric>|<zeros>|<lhs>|<rhs>|<mantissa>|<i>|<line>              writeCoordinates
 *   pm|<metric>|<mode>|<size>|<depth>|<angle>|<side>|<text>               writePostMachiningComment
 *   lin|<metric>|<value>|<text>                                           formatLinearValue
 *   header|<metric>|<zeros>|<minimal>|<type>|<line>                        writeEXCELLONHeader
 *   board|<name>|<copper>|<file name>|<own>
 *   layer|<id>|<name>          copper layers, UIOrder, BOARD::GetLayerName
 *   aux|<x>|<y>                the aux (drill / place file) origin
 *   span|<index>|<start>|<end>|<backdrill>|<npth>
 *   case|<span>|<merge>|<npth>, rawhash|<fnv>, and for own boards raw|<op> (tests/drill_oracle.rs)
 *   combo|<k>|<metric>|<zeros>|<mirror>|<minimal>|<combine>|<route>|<plot origin>|<full>
 *     msg|<severity>|<text>     REPORTER messages
 *     created|<i>,<j>,...       GetCreatedFiles() as indices into the "Created file" order
 *     file|<bytes>|<fnv>        files in creation ("Created file") order, then for full
 *     line|<text>               combos the lines of the file
 *   report|<merge>|<path>|<ok>|<bytes>|<fnv>, then line|<text>
 *   end|<name>
 */

#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <ctime>
#include <fstream>
#include <algorithm>
#include <map>
#include <sstream>
#include <string>
#include <vector>
#include <unistd.h>
#include <sys/stat.h>

#include <wx/app.h>
#include <wx/init.h>
#include <wx/filename.h>
#include <wx/log.h>

#include <kiplatform/app.h>
#include <mock_pgm_base.h>
#include <settings/settings_manager.h>
#include <project.h>
#include <pcbnew_settings.h>
#include <qa_utils/mock_utils.h>

#include <board.h>
#include <board_design_settings.h>
#include <board_loader.h>
#include <build_version.h>
#include <footprint.h>
#include <pad.h>
#include <pcb_track.h>
#include <reporter.h>
#include <pcb_io/pcb_io_mgr.h>
#include <wildcards_and_files_ext.h>
#include <kiway_player.h>

#include <drill/drill_enumerator.h>
#include <exporters/gendrill_writer_base.h>

// The private helpers of EXCELLON_WRITER are called directly (test driver only; the class
// layout does not depend on access specifiers).
#define private public
#include <exporters/gendrill_excellon_writer.h>
#undef private


/// The pinned clock: 2025-11-09 18:04:05 UTC (23:34:05 at +05:30).
static const time_t PINNED_TIME = 1762711445;

extern "C" time_t time( time_t* aOut )
{
    if( aOut )
        *aOut = PINNED_TIME;

    return PINNED_TIME;
}


static uint64_t fnv1a( const std::string& aText )
{
    uint64_t h = 0xcbf29ce484222325ULL;

    for( unsigned char c : aText )
    {
        h ^= c;
        h *= 0x100000001b3ULL;
    }

    return h;
}


static std::string hex( uint64_t aValue )
{
    char buf[32];
    snprintf( buf, sizeof( buf ), "%016llx", (unsigned long long) aValue );
    return buf;
}


static std::string readFile( const std::string& aPath )
{
    std::ifstream     in( aPath, std::ios::binary );
    std::stringstream ss;
    ss << in.rdbuf();
    return ss.str();
}


/// Prints the lines of a text (each must end with '\n'; a missing final newline is flagged).
static void printLines( const std::string& aText )
{
    size_t start = 0;

    while( start < aText.size() )
    {
        size_t nl = aText.find( '\n', start );

        if( nl == std::string::npos )
        {
            printf( "lastline|%s\n", aText.substr( start ).c_str() );
            break;
        }

        printf( "line|%s\n", aText.substr( start, nl - start ).c_str() );
        start = nl + 1;
    }
}


/// Records every message with its severity.
class RECORDING_REPORTER : public REPORTER
{
public:
    REPORTER& Report( const wxString& aText, SEVERITY aSeverity ) override
    {
        REPORTER::Report( aText, aSeverity );
        m_messages.emplace_back( (int) aSeverity, aText.ToStdString() );
        return *this;
    }

    bool HasMessage() const override { return !m_messages.empty(); }

    std::vector<std::pair<int, std::string>> m_messages;
};


/// Exposes the protected members of the base writer (for the raw enumeration lists).
class ORACLE_WRITER : public GENDRILL_WRITER_BASE
{
public:
    ORACLE_WRITER( BOARD* aPcb ) : GENDRILL_WRITER_BASE( aPcb ) {}
};


static std::string fmtOpt( const std::optional<int>& aVal )
{
    return aVal ? std::to_string( *aVal ) : std::string( "-" );
}


/// Source item ordinals as in tests/oracle/drill_oracle.cpp.
static std::map<const BOARD_ITEM*, std::string> s_ordinals;


static void numberItems( BOARD* aBoard )
{
    s_ordinals.clear();
    int n = 0;

    for( PCB_TRACK* track : aBoard->Tracks() )
        s_ordinals[track] = "V" + std::to_string( n++ );

    n = 0;

    for( FOOTPRINT* fp : aBoard->Footprints() )
    {
        for( PAD* pad : fp->Pads() )
            s_ordinals[pad] = "P" + std::to_string( n++ );
    }
}


/// Same format as fmtOp of tests/oracle/drill_oracle.cpp.
static std::string fmtOp( const DRILL_OPERATION& op )
{
    std::string src = "-";

    if( s_ordinals.count( op.m_SourceItem ) )
        src = s_ordinals[op.m_SourceItem];

    if( op.m_SourceItem && op.m_SourceItem->m_Uuid != op.m_SourceId )
        src += "!uuid";

    char buf[1024];
    snprintf( buf, sizeof( buf ),
              "%d|%s|%d|%d|%d|%d|%.17g|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|%d|"
              "%d|%d|%d|%d|%d|%d|%d|%d|%s",
              (int) op.m_Kind, src.c_str(), op.m_Position.x,
              op.m_Position.y, op.m_SizeXY.x, op.m_SizeXY.y, op.m_Orientation.AsDegrees(),
              op.m_Diameter, (int) op.m_IsSlot, (int) op.m_TopLayer, (int) op.m_BottomLayer,
              (int) op.m_DrillStart, (int) op.m_DrillEnd, (int) op.m_NotPlated,
              (int) op.m_Attribute, (int) op.m_Filled, (int) op.m_Capped, (int) op.m_TopCovered,
              (int) op.m_BottomCovered, (int) op.m_TopPlugged, (int) op.m_BottomPlugged,
              (int) op.m_TopTented, (int) op.m_BottomTented,
              (int) op.m_FrontPostMachining.m_Mode, op.m_FrontPostMachining.m_Size,
              op.m_FrontPostMachining.m_Depth, op.m_FrontPostMachining.m_Angle,
              (int) op.m_BackPostMachining.m_Mode, op.m_BackPostMachining.m_Size,
              op.m_BackPostMachining.m_Depth, op.m_BackPostMachining.m_Angle,
              fmtOpt( op.m_StubLength ).c_str() );
    return buf;
}


/// One option set of the drill job (the Excellon half of JobExportDrill).
struct COMBO
{
    bool                        metric;
    EXCELLON_WRITER::ZEROS_FMT  zeros;
    bool                        mirror;
    bool                        minimal;
    bool                        combine;
    bool                        route;
    bool                        plotOrigin;
    bool                        full;
};


static const COMBO COMBOS[] = {
    // CLI defaults (pcb export drill): mm, decimal, merged, G85 slots
    { true, EXCELLON_WRITER::DECIMAL_FORMAT, false, false, true, false, false, true },
    { false, EXCELLON_WRITER::DECIMAL_FORMAT, false, false, false, true, false, false },
    { true, EXCELLON_WRITER::SUPPRESS_LEADING, false, false, false, false, false, false },
    { false, EXCELLON_WRITER::SUPPRESS_LEADING, true, false, true, false, false, false },
    { true, EXCELLON_WRITER::SUPPRESS_TRAILING, false, false, true, true, false, false },
    { false, EXCELLON_WRITER::SUPPRESS_TRAILING, false, false, false, false, false, false },
    { true, EXCELLON_WRITER::KEEP_ZEROS, true, false, false, false, false, false },
    { false, EXCELLON_WRITER::KEEP_ZEROS, false, false, true, false, false, false },
    { true, EXCELLON_WRITER::DECIMAL_FORMAT, false, true, false, true, false, false },
    { true, EXCELLON_WRITER::DECIMAL_FORMAT, true, false, true, false, true, false },
    { false, EXCELLON_WRITER::DECIMAL_FORMAT, false, true, true, false, true, false },
    { true, EXCELLON_WRITER::DECIMAL_FORMAT, false, false, false, false, false, false },
};


static void runCombo( BOARD* aBoard, int aIndex, const COMBO& c )
{
    printf( "combo|%d|%d|%d|%d|%d|%d|%d|%d|%d\n", aIndex, (int) c.metric, (int) c.zeros,
            (int) c.mirror, (int) c.minimal, (int) c.combine, (int) c.route, (int) c.plotOrigin,
            (int) c.full );

    // A fresh output directory "out" in the current (scratch) directory.
    system( "rm -rf out" );
    mkdir( "out", 0755 );

    // PCBNEW_JOBS_HANDLER::JobExportDrill, Excellon branch (pcbnew_jobs_handler.cpp
    // L2342-2425), without the job object.
    EXCELLON_WRITER writer( aBoard );
    VECTOR2I        offset( 0, 0 );

    if( c.plotOrigin )
        offset = aBoard->GetDesignSettings().GetAuxOrigin();

    DRILL_PRECISION precision = c.metric ? DRILL_PRECISION( 3, 3 ) : DRILL_PRECISION( 2, 4 );
    writer.SetFormat( c.metric, c.zeros, precision.m_Lhs, precision.m_Rhs );
    writer.SetOptions( c.mirror, c.minimal, offset, c.combine );
    writer.SetRouteModeForOvalHoles( c.route );
    writer.SetMapFileFormat( PLOT_FORMAT::PDF );

    RECORDING_REPORTER reporter;
    bool ok = writer.CreateDrillandMapFilesSet( wxT( "out" ), true, false, &reporter );
    printf( "result|%d\n", (int) ok );

    std::vector<std::string> createdOrder;

    for( const auto& [sev, text] : reporter.m_messages )
    {
        printf( "msg|%d|%s\n", sev, text.c_str() );

        const std::string prefix = "Created file '";

        if( text.rfind( prefix, 0 ) == 0 && text.back() == '\'' )
            createdOrder.push_back( text.substr( prefix.size(), text.size() - prefix.size() - 1 ) );
    }

    // GetCreatedFiles() as indices into the "Created file" order
    std::string created;

    for( const wxString& f : writer.GetCreatedFiles() )
    {
        size_t idx = std::find( createdOrder.begin(), createdOrder.end(), f.ToStdString() )
                     - createdOrder.begin();
        created += ( created.empty() ? "" : "," ) + std::to_string( idx );
    }

    printf( "created|%s\n", created.c_str() );

    for( const std::string& path : createdOrder )
    {
        std::string text = readFile( path );
        printf( "file|%zu|%s\n", text.size(), hex( fnv1a( text ) ).c_str() );

        if( c.full )
            printLines( text );
    }
}


static void runReport( BOARD* aBoard, bool aMerge )
{
    system( "rm -rf out" );
    mkdir( "out", 0755 );

    EXCELLON_WRITER writer( aBoard );
    writer.SetFormat( true, EXCELLON_WRITER::DECIMAL_FORMAT, 3, 3 );
    writer.SetOptions( false, false, VECTOR2I( 0, 0 ), aMerge );

    wxFileName fn = wxFileName::DirName( wxT( "out" ) );
    fn.SetFullName( aBoard->GetFileName() );
    fn.SetName( fn.GetName() + "-drill" );
    fn.SetExt( FILEEXT::ReportFileExtension );
    std::string path = fn.GetFullPath().ToStdString();

    bool        ok = writer.GenDrillReportFile( fn.GetFullPath() );
    std::string text = readFile( path );
    printf( "report|%d|%s|%d|%zu|%s\n", (int) aMerge, path.c_str(), (int) ok, text.size(),
            hex( fnv1a( text ) ).c_str() );
    printLines( text );
}


static void dumpBoard( BOARD* aBoard, const std::string& aName, bool aOwn )
{
    ORACLE_WRITER w( aBoard );
    numberItems( aBoard );

    printf( "board|%s|%d|%s|%d\n", aName.c_str(), aBoard->GetCopperLayerCount(),
            wxFileName( aBoard->GetFileName() ).GetFullName().ToStdString().c_str(), (int) aOwn );

    for( PCB_LAYER_ID layer : LSET::AllCuMask( aBoard->GetCopperLayerCount() ).UIOrder() )
        printf( "layer|%d|%s\n", (int) layer, aBoard->GetLayerName( layer ).ToStdString().c_str() );

    VECTOR2I aux = aBoard->GetDesignSettings().GetAuxOrigin();
    printf( "aux|%d|%d\n", aux.x, aux.y );

    std::vector<DRILL_SPAN> spans = EnumerateDrillSpans( *aBoard );
    spans.emplace_back( F_Cu, B_Cu, false, true );

    for( size_t si = 0; si < spans.size(); si++ )
    {
        const DRILL_SPAN& span = spans[si];
        printf( "span|%zu|%d|%d|%d|%d\n", si, (int) span.m_StartLayer, (int) span.m_EndLayer,
                (int) span.m_IsBackdrill, (int) span.m_IsNonPlatedFile );

        for( int merge = 0; merge < 2; merge++ )
        {
            for( int npth = 0; npth < 2; npth++ )
            {
                printf( "case|%zu|%d|%d\n", si, merge, npth );

                DRILL_QUERY query;
                query.m_Span = span;
                query.m_NonPlatedOnly = npth;
                query.m_MergePTHNPTH = merge;

                std::string all;

                for( const DRILL_OPERATION& op : EnumerateDrillOperations( *aBoard, query ) )
                {
                    std::string text = fmtOp( op );
                    all += text + "\n";

                    if( aOwn )
                        printf( "raw|%s\n", text.c_str() );
                }

                printf( "rawhash|%s\n", hex( fnv1a( all ) ).c_str() );
            }
        }
    }

    int k = 0;

    for( const COMBO& c : COMBOS )
        runCombo( aBoard, k++, c );

    runReport( aBoard, true );
    runReport( aBoard, false );

    printf( "end|%s\n", aName.c_str() );
}


/// Access to protected base members (pointer-to-member through a derived class).
struct EXPOSED : EXCELLON_WRITER
{
    static double& Conv( EXCELLON_WRITER& w ) { return w.*( &EXPOSED::m_conversionUnits ); }
    static DRILL_PRECISION& Prec( EXCELLON_WRITER& w ) { return w.*( &EXPOSED::m_precision ); }
};


/// Coordinates for writeCoordinates: board coordinates (IU) converted as createDrillFile
/// does, plus raw doubles at the rounding boundaries.
static std::vector<std::pair<double, double>> coordValues( EXCELLON_WRITER& w )
{
    const int ius[] = { 0, 1, -1, 4, 5, -5, 6, 49, 50, 51, -50, 250, 499, 500, 501, -500,
                        1270, 2540, 12700, 25400, 63500, 127000, 1234567, -1234567, 148000000,
                        -148000000, 999999999, -999999999, 2147483647, -2147483647 };
    const double raws[] = { 0.0, -0.0, 0.5, -0.5, 0.0005, -0.0005, 0.00049999999999999, 1.0005,
                            1.00049999, 2.5e-4, 0.1, 123.4565, -123.4565, 99.9995, 1e9, -1e9,
                            3e6, -3e6, 1e-12, -1e-12, 0.05, 0.95, 9.9999 };

    std::vector<std::pair<double, double>> values;

    for( size_t i = 0; i < std::size( ius ); i++ )
    {
        int x = ius[i];
        int y = ius[( i * 7 + 3 ) % std::size( ius )];
        values.emplace_back( x * EXPOSED::Conv( w ), y * EXPOSED::Conv( w ) );
    }

    for( size_t i = 0; i < std::size( raws ); i++ )
        values.emplace_back( raws[i], raws[( i * 5 + 2 ) % std::size( raws )] );

    return values;
}


/// Direct checks of the number formatting helpers.
static void dumpFormatting( BOARD* aBoard )
{
    EXCELLON_WRITER w( aBoard );

    // The value table per unit (coord lines refer to it by index).
    for( int metric = 1; metric >= 0; metric-- )
    {
        w.SetFormat( metric, EXCELLON_WRITER::DECIMAL_FORMAT, 0, 0 );
        std::vector<std::pair<double, double>> values = coordValues( w );

        for( size_t i = 0; i < values.size(); i++ )
            printf( "val|%d|%zu|%.17g|%.17g\n", metric, i, values[i].first, values[i].second );
    }

    for( int metric = 1; metric >= 0; metric-- )
    {
        for( int zeros = 0; zeros < 4; zeros++ )
        {
            const int prec[][2] = { { 0, 0 }, { 3, 3 }, { 2, 4 }, { 4, 2 }, { 1, 5 } };

            for( const auto& p : prec )
            {
                w.SetFormat( metric, (EXCELLON_WRITER::ZEROS_FMT) zeros, p[0], p[1] );
                w.m_mantissaLenght = metric ? 3 : 4;

                std::vector<std::pair<double, double>> values = coordValues( w );

                for( size_t i = 0; i < values.size(); i++ )
                {
                    char line[1024];
                    w.writeCoordinates( line, sizeof( line ), values[i].first, values[i].second );
                    std::string s( line );

                    if( !s.empty() && s.back() == '\n' )
                        s.pop_back();

                    printf( "coord|%d|%d|%d|%d|%d|%zu|%s\n", metric, zeros,
                            EXPOSED::Prec( w ).m_Lhs, EXPOSED::Prec( w ).m_Rhs,
                            w.m_mantissaLenght, i, s.c_str() );
                }
            }
        }
    }

    // Post-machining comments and linear values.
    for( int metric = 1; metric >= 0; metric-- )
    {
        w.SetFormat( metric, EXCELLON_WRITER::DECIMAL_FORMAT, 0, 0 );
        w.m_mantissaLenght = metric ? 3 : 4;

        const int linear[] = { -5, 0, 1, 499, 500, 600000, 150000, 1234567, 25400 };

        for( int v : linear )
            printf( "lin|%d|%d|%s\n", metric, v, w.formatLinearValue( v ).ToStdString().c_str() );

        const PAD_DRILL_POST_MACHINING_MODE modes[] = {
            PAD_DRILL_POST_MACHINING_MODE::UNKNOWN,
            PAD_DRILL_POST_MACHINING_MODE::NOT_POST_MACHINED,
            PAD_DRILL_POST_MACHINING_MODE::COUNTERBORE,
            PAD_DRILL_POST_MACHINING_MODE::COUNTERSINK
        };
        const int values[][3] = { { 600000, 150000, 900 }, { 0, 0, 0 },       { 700000, 0, 825 },
                                  { 0, 200000, 1 },        { -1, -1, -900 },  { 1500000, 1, 3599 } };

        for( PAD_DRILL_POST_MACHINING_MODE mode : modes )
        {
            for( const auto& v : values )
            {
                for( const char* side : { "front", "back" } )
                {
                    char*  buf = nullptr;
                    size_t len = 0;
                    FILE*  mem = open_memstream( &buf, &len );
                    w.m_file = mem;
                    w.writePostMachiningComment( mode, v[0], v[1], v[2], wxString( side ) );
                    fclose( mem );
                    std::string text( buf, len );
                    free( buf );

                    if( !text.empty() && text.back() == '\n' )
                        text.pop_back();

                    printf( "pm|%d|%d|%d|%d|%d|%s|%s\n", metric, (int) mode, v[0], v[1], v[2],
                            side, text.c_str() );
                }
            }
        }
    }

    // Headers: every unit / zero format / header mode for the through span and each file type.
    DRILL_SPAN span( F_Cu, B_Cu, false, false );

    for( int metric = 1; metric >= 0; metric-- )
    {
        for( int zeros = 0; zeros < 4; zeros++ )
        {
            for( int minimal = 0; minimal < 2; minimal++ )
            {
                for( int type = 0; type < 3; type++ )
                {
                    w.SetFormat( metric, (EXCELLON_WRITER::ZEROS_FMT) zeros, 0, 0 );
                    w.SetOptions( false, minimal, VECTOR2I( 0, 0 ), false );

                    char*  buf = nullptr;
                    size_t len = 0;
                    FILE*  mem = open_memstream( &buf, &len );
                    w.m_file = mem;
                    w.writeEXCELLONHeader( span, (GENDRILL_WRITER_BASE::TYPE_FILE) type );
                    fclose( mem );
                    std::string text( buf, len );
                    free( buf );

                    size_t start = 0;

                    while( start < text.size() )
                    {
                        size_t nl = text.find( '\n', start );
                        printf( "header|%d|%d|%d|%d|%s\n", metric, zeros, minimal, type,
                                text.substr( start, nl - start ).c_str() );
                        start = nl + 1;
                    }
                }
            }
        }
    }
}


/// Deterministic generator for the synthetic boards (numerical recipes LCG).
struct LCG
{
    uint32_t s;
    uint32_t next() { s = s * 1664525u + 1013904223u; return s >> 8; }
    int      pick( int n ) { return (int) ( next() % (uint32_t) n ); }
};


/// Verbatim copy of makeSynthetic of tests/oracle/drill_oracle.cpp (same boards, so that the
/// FAB-01 golden's raw lists apply; rawhash checks it).
static std::unique_ptr<BOARD> makeSynthetic( const std::string& aName, int aCopper, uint32_t aSeed,
                                             int aVias, int aPads )
{
    KIID::SeedGenerator( aSeed );
    LCG rng{ aSeed };

    auto board = std::make_unique<BOARD>();
    board->SetFileName( wxString::FromUTF8( "/synthetic/" + aName + ".kicad_pcb" ) );
    board->SetCopperLayerCount( aCopper );
    BOARD_DESIGN_SETTINGS& bds = board->GetDesignSettings();
    bds.GetStackupDescriptor().BuildDefaultStackupList( &bds, aCopper );

    LSEQ cu = LSET::AllCuMask( aCopper ).UIOrder(); // F_Cu, In1 ... , B_Cu
    const int mm = 1000000;

    auto layerAt = [&]( int i ) { return cu[std::min<int>( i, cu.size() - 1 )]; };

    for( int i = 0; i < aVias; i++ )
    {
        PCB_VIA* via = new PCB_VIA( board.get() );
        via->SetPosition( VECTOR2I( ( 1 + rng.pick( 3 ) ) * mm, ( 1 + rng.pick( 3 ) ) * mm ) );
        via->SetWidth( PADSTACK::ALL_LAYERS, mm );
        int drills[] = { 300000, 400000, 400000 };
        via->SetDrill( drills[rng.pick( 3 )] );

        int type = rng.pick( 4 );

        if( type == 0 || aCopper < 4 )
        {
            via->SetViaType( VIATYPE::THROUGH );
            via->SetLayerPair( F_Cu, B_Cu );
        }
        else if( type == 1 )
        {
            via->SetViaType( VIATYPE::BLIND );
            if( rng.pick( 2 ) )
                via->SetLayerPair( F_Cu, layerAt( 2 ) );
            else
                via->SetLayerPair( layerAt( 2 ), B_Cu );
        }
        else if( type == 2 )
        {
            via->SetViaType( VIATYPE::BURIED );
            via->SetLayerPair( layerAt( 1 ), layerAt( 3 ) );
        }
        else
        {
            via->SetViaType( VIATYPE::MICROVIA );
            via->SetLayerPair( F_Cu, layerAt( 1 ) );
        }

        int pm = rng.pick( 4 );

        if( pm == 1 )
        {
            via->SetFrontPostMachining( PAD_DRILL_POST_MACHINING_MODE::COUNTERSINK );
            via->SetFrontPostMachiningSize( 600000 );
            via->SetFrontPostMachiningDepth( 150000 );
            via->SetFrontPostMachiningAngle( 900 );
        }
        else if( pm == 2 )
        {
            via->Padstack().BackPostMachining().mode = PAD_DRILL_POST_MACHINING_MODE::COUNTERBORE;
            via->Padstack().BackPostMachining().size = 700000;
            via->Padstack().BackPostMachining().depth = 200000;
        }

        if( aCopper >= 4 && via->GetViaType() == VIATYPE::THROUGH )
        {
            int bd = rng.pick( 4 );

            if( bd == 1 || bd == 3 )
            {
                via->SetSecondaryDrillSize( VECTOR2I( rng.pick( 2 ) ? 500000 : 600000, 0 ) );
                via->SetSecondaryDrillStartLayer( B_Cu );
                via->SetSecondaryDrillEndLayer( layerAt( 2 ) );
            }

            if( bd == 2 || bd == 3 )
            {
                via->SetTertiaryDrillSize( VECTOR2I( 500000, 500000 ) );
                via->SetTertiaryDrillStartLayer( F_Cu );
                via->SetTertiaryDrillEndLayer( layerAt( 1 ) );
            }
        }

        const TENTING_MODE  tm[] = { TENTING_MODE::FROM_BOARD, TENTING_MODE::TENTED, TENTING_MODE::NOT_TENTED };
        const COVERING_MODE cm[] = { COVERING_MODE::FROM_BOARD, COVERING_MODE::COVERED, COVERING_MODE::NOT_COVERED };
        const PLUGGING_MODE gm[] = { PLUGGING_MODE::FROM_BOARD, PLUGGING_MODE::PLUGGED, PLUGGING_MODE::NOT_PLUGGED };
        const CAPPING_MODE  am[] = { CAPPING_MODE::FROM_BOARD, CAPPING_MODE::CAPPED, CAPPING_MODE::NOT_CAPPED };
        const FILLING_MODE  fm[] = { FILLING_MODE::FROM_BOARD, FILLING_MODE::FILLED, FILLING_MODE::NOT_FILLED };
        via->SetFrontTentingMode( tm[rng.pick( 3 )] );
        via->SetBackTentingMode( tm[rng.pick( 3 )] );
        via->SetFrontCoveringMode( cm[rng.pick( 3 )] );
        via->SetBackCoveringMode( cm[rng.pick( 3 )] );
        via->SetFrontPluggingMode( gm[rng.pick( 3 )] );
        via->SetBackPluggingMode( gm[rng.pick( 3 )] );
        via->SetCappingMode( am[rng.pick( 3 )] );
        via->SetFillingMode( fm[rng.pick( 3 )] );

        board->Add( via, ADD_MODE::APPEND );
    }

    FOOTPRINT* fp = new FOOTPRINT( board.get() );
    board->Add( fp, ADD_MODE::APPEND );

    for( int i = 0; i < aPads; i++ )
    {
        PAD* pad = new PAD( fp );
        const PAD_ATTRIB attrs[] = { PAD_ATTRIB::PTH, PAD_ATTRIB::PTH, PAD_ATTRIB::NPTH, PAD_ATTRIB::SMD };
        PAD_ATTRIB attr = attrs[rng.pick( 4 )];
        pad->SetAttribute( attr );
        pad->SetSize( PADSTACK::ALL_LAYERS, VECTOR2I( 3 * mm, 3 * mm ) );

        if( attr == PAD_ATTRIB::SMD )
        {
            pad->SetLayerSet( PAD::SMDMask() );
            pad->SetDrillSize( VECTOR2I( 0, 0 ) );
        }
        else
        {
            pad->SetLayerSet( attr == PAD_ATTRIB::NPTH ? PAD::UnplatedHoleMask() : PAD::PTHMask() );
            const VECTOR2I sizes[] = { { mm, mm }, { mm, 2 * mm }, { 2 * mm, mm }, { 800000, 800000 } };
            VECTOR2I sz = sizes[rng.pick( 4 )];
            pad->SetDrillSize( sz );
            pad->SetDrillShape( sz.x != sz.y || rng.pick( 2 ) ? PAD_DRILL_SHAPE::OBLONG
                                                              : PAD_DRILL_SHAPE::CIRCLE );
        }

        const PAD_PROP props[] = { PAD_PROP::NONE, PAD_PROP::CASTELLATED, PAD_PROP::PRESSFIT };
        pad->SetProperty( props[rng.pick( 3 )] );
        pad->SetPosition( VECTOR2I( ( 10 + rng.pick( 3 ) ) * mm, ( 1 + rng.pick( 2 ) ) * mm ) );
        const double angles[] = { 0.0, 90.0, 45.0 };
        pad->SetOrientation( EDA_ANGLE( angles[rng.pick( 3 )], DEGREES_T ) );

        if( rng.pick( 3 ) == 0 )
        {
            pad->SetFrontPostMachining( PAD_DRILL_POST_MACHINING_MODE::COUNTERSINK );
            pad->Padstack().FrontPostMachining().size = 1500000;
        }

        fp->Add( pad, ADD_MODE::APPEND );
    }

    return board;
}


/// The boards of qa/tests/pcbnew/test_cam_backdrill.cpp that use the Excellon writer or the
/// report (BackdrillCamOutputs, FrontAndBackBackdrillCamOutputs, DualBackdrillSameViaCamOutputs,
/// DrillFileLayerOrderInFilename, DrillReportNullReporter, DrillReportWithTools), built the
/// same way; plus slots at odd angles for the routing code.
static std::unique_ptr<BOARD> makeCamBoard( int aWhich, std::string& aName )
{
    auto board = std::make_unique<BOARD>();
    auto mmIU = []( double mm ) { return pcbIUScale.mmToIU( mm ); };

    auto addVia = [&]( VECTOR2I pos ) -> PCB_VIA*
    {
        PCB_VIA* via = new PCB_VIA( board.get() );
        via->SetPadstackMode( PADSTACK::MODE::NORMAL );
        via->SetPosition( pos );
        via->SetLayerPair( F_Cu, B_Cu );
        via->SetDrill( mmIU( 0.30 ) );
        via->SetWidth( PADSTACK::ALL_LAYERS, mmIU( 0.60 ) );
        board->Add( via );
        return via;
    };

    switch( aWhich )
    {
    case 0:
    {
        aName = "backdrill_board";
        board->SetCopperLayerCount( 6 );
        PCB_VIA* via = addVia( VECTOR2I( 0, 0 ) );
        via->SetSecondaryDrillSize( mmIU( 0.20 ) );
        via->SetSecondaryDrillStartLayer( F_Cu );
        via->SetSecondaryDrillEndLayer( In3_Cu );
        via->SetFrontPostMachiningMode( PAD_DRILL_POST_MACHINING_MODE::COUNTERSINK );
        via->SetFrontPostMachiningSize( mmIU( 0.60 ) );
        via->SetFrontPostMachiningDepth( mmIU( 0.15 ) );
        via->SetFrontPostMachiningAngle( 900 );
        break;
    }
    case 1:
    {
        aName = "backdrill_pair_board";
        board->SetCopperLayerCount( 6 );
        PCB_VIA* topVia = addVia( VECTOR2I( 0, 0 ) );
        topVia->SetSecondaryDrillSize( mmIU( 0.40 ) );
        topVia->SetSecondaryDrillStartLayer( F_Cu );
        topVia->SetSecondaryDrillEndLayer( In1_Cu );
        PCB_VIA* bottomVia = addVia( VECTOR2I( mmIU( 5.0 ), 0 ) );
        bottomVia->SetTertiaryDrillSize( mmIU( 0.40 ) );
        bottomVia->SetTertiaryDrillStartLayer( B_Cu );
        bottomVia->SetTertiaryDrillEndLayer( In3_Cu );
        break;
    }
    case 2:
    {
        aName = "dual_backdrill_board";
        board->SetCopperLayerCount( 6 );
        PCB_VIA* via = addVia( VECTOR2I( 0, 0 ) );
        via->SetSecondaryDrillSize( mmIU( 0.40 ) );
        via->SetSecondaryDrillStartLayer( F_Cu );
        via->SetSecondaryDrillEndLayer( In1_Cu );
        via->SetTertiaryDrillSize( mmIU( 0.40 ) );
        via->SetTertiaryDrillStartLayer( B_Cu );
        via->SetTertiaryDrillEndLayer( In3_Cu );
        break;
    }
    case 3:
    {
        aName = "layer_order_board";
        board->SetCopperLayerCount( 4 );
        PCB_VIA* via = addVia( VECTOR2I( 0, 0 ) );
        via->SetViaType( VIATYPE::BURIED );
        via->SetLayerPair( In2_Cu, B_Cu );
        break;
    }
    case 4:
        aName = "test_board";
        board->SetCopperLayerCount( 2 );
        break;

    case 5:
        aName = "test_board_with_drills";
        board->SetCopperLayerCount( 2 );
        addVia( VECTOR2I( 0, 0 ) );
        break;

    case 6:
    {
        // Slots at odd angles and sizes, negative coordinates, user layer names.
        aName = "slots_board";
        board->SetCopperLayerCount( 2 );
        board->SetLayerName( F_Cu, wxT( "Top copper" ) );
        board->SetLayerName( B_Cu, wxString::FromUTF8( "Bottom \xe5\xba\x95\xe5\xb1\x82 layer" ) );
        FOOTPRINT* fp = new FOOTPRINT( board.get() );
        board->Add( fp, ADD_MODE::APPEND );
        const double angles[] = { 0.0, 30.0, 45.0, 90.0, 135.5, 180.0, 270.0, 333.3, -15.0 };
        int          i = 0;

        for( double a : angles )
        {
            for( int plated = 0; plated < 2; plated++ )
            {
                PAD* pad = new PAD( fp );
                pad->SetAttribute( plated ? PAD_ATTRIB::PTH : PAD_ATTRIB::NPTH );
                pad->SetLayerSet( plated ? PAD::PTHMask() : PAD::UnplatedHoleMask() );
                pad->SetSize( PADSTACK::ALL_LAYERS, VECTOR2I( mmIU( 4 ), mmIU( 4 ) ) );
                VECTOR2I sz = ( i % 2 ) ? VECTOR2I( 1234567, 654321 ) : VECTOR2I( 700001, 2100003 );
                pad->SetDrillSize( sz );
                pad->SetDrillShape( PAD_DRILL_SHAPE::OBLONG );
                pad->SetPosition( VECTOR2I( -12345678 + i * 3333333, 9876543 - i * 1111111 ) );
                pad->SetOrientation( EDA_ANGLE( a, DEGREES_T ) );
                pad->SetProperty( i % 3 == 0 ? PAD_PROP::CASTELLATED
                                             : ( i % 3 == 1 ? PAD_PROP::PRESSFIT : PAD_PROP::NONE ) );
                fp->Add( pad, ADD_MODE::APPEND );
                i++;
            }
        }

        // A round hole of the same size as a slot tool, and a slot of zero minor size
        PAD* round = new PAD( fp );
        round->SetAttribute( PAD_ATTRIB::PTH );
        round->SetLayerSet( PAD::PTHMask() );
        round->SetSize( PADSTACK::ALL_LAYERS, VECTOR2I( mmIU( 4 ), mmIU( 4 ) ) );
        round->SetDrillSize( VECTOR2I( 654321, 654321 ) );
        round->SetPosition( VECTOR2I( 1, -1 ) );
        fp->Add( round, ADD_MODE::APPEND );

        board->GetDesignSettings().SetAuxOrigin( VECTOR2I( -2500000, 3500001 ) );
        break;
    }
    }

    board->SetFileName( wxString::FromUTF8( "/cam/" + aName + ".kicad_pcb" ) );
    return board;
}


static bool initPgm( int argc, char** argv )
{
    KI_TEST::SetMockConfigDir();
    SetPgm( new MOCK_PGM_BASE() );
    KIPLATFORM::APP::Init();
    wxApp::SetInstance( new wxAppConsole );

    if( !wxInitialize( argc, argv ) )
        return false;

    wxLog::EnableLogging( false );
    Pgm().InitPgm( true, true );
    Pgm().GetSettingsManager().RegisterSettings( new PCBNEW_SETTINGS, false );
    Pgm().GetSettingsManager().Load();
    Pgm().GetSettingsManager().LoadProject( "" );
    return true;
}


/// kicad-cli load path (PCBNEW_JOBS_HANDLER::getBoard).
static std::unique_ptr<BOARD> loadBoardCli( const wxString& aPath )
{
    SETTINGS_MANAGER& mgr = Pgm().GetSettingsManager();
    wxFileName        pro( aPath );
    pro.SetExt( FILEEXT::ProjectFileExtension );
    pro.MakeAbsolute();

    PROJECT* project = mgr.GetProject( pro.GetFullPath() );

    if( !project )
    {
        mgr.LoadProject( pro.GetFullPath(), true );
        project = mgr.GetProject( pro.GetFullPath() );
    }

    PCB_IO_MGR::PCB_FILE_T type = PCB_IO_MGR::FindPluginTypeFromBoardPath( aPath, KICTL_KICAD_ONLY );
    return BOARD_LOADER::Load( aPath, type, project );
}


static void noAssert( const wxString&, int, const wxString&, const wxString&, const wxString& )
{
}


/// Usage: excellon-oracle <scratch dir> <board>...  (boards given as absolute paths)
int main( int argc, char** argv )
{
    setenv( "LC_ALL", "C.UTF-8", 1 );
    setenv( "TZ", "XST-5:30", 1 );
    tzset();
    unsetenv( "DISPLAY" );
    unsetenv( "WAYLAND_DISPLAY" );

    if( argc < 2 || !initPgm( 1, argv ) )
        return 1;

    wxSetAssertHandler( &noAssert );

    if( chdir( argv[1] ) != 0 )
        return 2;

    printf( "version|%s\n", GetBuildVersion().ToStdString().c_str() );

    {
        BOARD fmtBoard;
        fmtBoard.SetFileName( wxT( "/fmt/fmt.kicad_pcb" ) );
        dumpFormatting( &fmtBoard );
    }

    // Synthetic boards of the FAB-01 oracle, then the CAM test boards, then the corpus.
    struct SYN { const char* name; int copper; uint32_t seed; int vias; int pads; };
    const SYN syn[] = { { "synthetic6", 6, 12345, 100, 40 }, { "synthetic4", 4, 777, 30, 20 },
                        { "synthetic2", 2, 99, 24, 24 }, { "synthetic32", 32, 5, 30, 6 } };

    for( const SYN& s : syn )
    {
        std::unique_ptr<BOARD> board = makeSynthetic( s.name, s.copper, s.seed, s.vias, s.pads );
        board->GetDesignSettings().SetAuxOrigin( VECTOR2I( 1234567, -7654321 ) );
        dumpBoard( board.get(), s.name, false );
    }

    for( int i = 0; i < 7; i++ )
    {
        std::string            name;
        std::unique_ptr<BOARD> board = makeCamBoard( i, name );
        dumpBoard( board.get(), name, true );
    }

    for( int i = 2; i < argc; i++ )
    {
        wxString path = wxString::FromUTF8( argv[i] );
        std::unique_ptr<BOARD> board = loadBoardCli( path );

        if( !board )
        {
            printf( "error|load|%s\n", argv[i] );
            continue;
        }

        dumpBoard( board.get(), wxFileName( path ).GetName().ToStdString(), false );
        Pgm().GetSettingsManager().LoadProject( "" );
    }

    fflush( stdout );
    // Skip static destructors of the mocked program (as pcb-dump does).
    _exit( 0 );
}
