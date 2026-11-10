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

// Package M8 differential oracle for STROKE_PARAMS::Stroke (common/stroke_params.cpp): strokes
// a fixed set of shapes with every line style and prints, per case, the number of pieces and
// the FNV-1a hash of "ax,ay>bx,by " for all pieces (plus the first pieces in clear). The Rust
// replay is stroke_params::tests::stroke_matches_oracle.

#include <cstdio>
#include <string>
#include <vector>

#include <geometry/shape_arc.h>
#include <geometry/shape_ellipse.h>
#include <geometry/shape_line_chain.h>
#include <geometry/shape_rect.h>
#include <geometry/shape_segment.h>
#include <geometry/shape_simple.h>
#include <render_settings.h>
#include <stroke_params.h>


class ORACLE_RENDER_SETTINGS : public KIGFX::RENDER_SETTINGS
{
public:
    KIGFX::COLOR4D GetColor( const KIGFX::VIEW_ITEM* aItem, int aLayer ) const override
    {
        return KIGFX::COLOR4D::BLACK;
    }

    const KIGFX::COLOR4D& GetBackgroundColor() const override { return m_color; }
    void SetBackgroundColor( const KIGFX::COLOR4D& aColor ) override { m_color = aColor; }
    const KIGFX::COLOR4D& GetGridColor() override { return m_color; }
    const KIGFX::COLOR4D& GetCursorColor() override { return m_color; }

private:
    KIGFX::COLOR4D m_color = KIGFX::COLOR4D::BLACK;
};


static uint64_t fnv( const std::string& s )
{
    uint64_t h = 0xcbf29ce484222325ULL;

    for( unsigned char c : s )
    {
        h ^= c;
        h *= 0x100000001b3ULL;
    }

    return h;
}


int main()
{
    ORACLE_RENDER_SETTINGS rs;

    SHAPE_LINE_CHAIN chain;
    chain.Append( VECTOR2I( 0, 0 ) );
    chain.Append( VECTOR2I( 1000000, 0 ) );
    chain.Append( VECTOR2I( 1500000, 700000 ) );
    chain.Append( VECTOR2I( 1500000, 700000 ) );
    chain.Append( VECTOR2I( 1700000, 2500000 ) );
    chain.Append( VECTOR2I( -300000, 2600000 ) );

    SHAPE_SIMPLE simple;
    simple.Append( VECTOR2I( 0, 0 ) );
    simple.Append( VECTOR2I( 3000000, 200000 ) );
    simple.Append( VECTOR2I( 1000000, 2000000 ) );

    SHAPE_RECT rrect( VECTOR2I( 0, 0 ), 5000000, 3000000 );
    rrect.SetRadius( 800000 );

    std::vector<std::pair<std::string, SHAPE*>> shapes = {
        { "seg", new SHAPE_SEGMENT( VECTOR2I( 100000, 200000 ), VECTOR2I( 5300000, 2900000 ) ) },
        { "seg_short", new SHAPE_SEGMENT( VECTOR2I( 0, 0 ), VECTOR2I( 0, -700000 ) ) },
        { "chain", &chain },
        { "simple", &simple },
        { "rect", new SHAPE_RECT( VECTOR2I( -1000000, 500000 ), 4000000, 2500000 ) },
        { "rrect", &rrect },
        { "arc", new SHAPE_ARC( VECTOR2I( 0, 0 ), VECTOR2I( 3000000, 0 ), EDA_ANGLE( 135.0, DEGREES_T ), 0 ) },
        { "arc_cw", new SHAPE_ARC( VECTOR2I( 1000000, 0 ), VECTOR2I( 1000000, 2000000 ), EDA_ANGLE( -200.0, DEGREES_T ), 0 ) },
        { "ring", new SHAPE_ARC( VECTOR2I( 0, 0 ), VECTOR2I( 2500000, 0 ), ANGLE_360, 0 ) },
        { "ellipse", new SHAPE_ELLIPSE( VECTOR2I( 500000, -300000 ), 4000000, 1500000, EDA_ANGLE( 25.0, DEGREES_T ) ) },
        { "ellipse_arc", new SHAPE_ELLIPSE( VECTOR2I( 0, 0 ), 3000000, 2000000, EDA_ANGLE( -10.0, DEGREES_T ),
                                            EDA_ANGLE( 200.0, DEGREES_T ), EDA_ANGLE( 30.0, DEGREES_T ) ) },
    };

    const std::vector<std::pair<std::string, LINE_STYLE>> styles = {
        { "solid", LINE_STYLE::SOLID },     { "dash", LINE_STYLE::DASH },
        { "dot", LINE_STYLE::DOT },         { "dashdot", LINE_STYLE::DASHDOT },
        { "dashdotdot", LINE_STYLE::DASHDOTDOT },
    };

    for( int width : { 100000, 37000 } )
    {
        for( const auto& [sname, shape] : shapes )
        {
            for( const auto& [lname, style] : styles )
            {
                std::string all, head;
                int         count = 0;

                STROKE_PARAMS::Stroke( shape, style, width, &rs,
                                       [&]( const VECTOR2I& a, const VECTOR2I& b )
                                       {
                                           std::string p = std::to_string( a.x ) + "," + std::to_string( a.y ) + ">"
                                                           + std::to_string( b.x ) + "," + std::to_string( b.y ) + " ";
                                           all += p;

                                           if( count < 3 )
                                               head += p;

                                           count++;
                                       } );

                printf( "%s|%s|%d|%d|%016llx|%s\n", sname.c_str(), lname.c_str(), width, count,
                        (unsigned long long) fnv( all ), head.c_str() );
            }
        }
    }

    return 0;
}
