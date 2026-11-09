// Research driver: sul::dynamic_bitset<uint64_t> (the base of KiCad's BASE_SET / LSET) on a
// random script of the operations BASE_SET uses.  Prints "count any none all bits(LSB first)".
#include <dynamic_bitset.h>

#include <cstdio>
#include <cstring>
#include <string>

int main( int argc, char** argv )
{
    if( argc < 3 )
        return 2;

    FILE* in = fopen( argv[1], "r" );
    FILE* out = fopen( argv[2], "w" );

    if( !in || !out )
        return 2;

    sul::dynamic_bitset<uint64_t> a, b;
    char                          op[8];
    size_t                        arg = 0;

    while( fscanf( in, "%7s %zu", op, &arg ) == 2 )
    {
        if( !strcmp( op, "N" ) )        { a = sul::dynamic_bitset<uint64_t>( arg ); b = sul::dynamic_bitset<uint64_t>( arg ); }
        else if( !strcmp( op, "S" ) )   a.set( arg );
        else if( !strcmp( op, "R" ) )   a.reset( arg );
        else if( !strcmp( op, "F" ) )   a.flip( arg );
        else if( !strcmp( op, "SA" ) )  a.set();
        else if( !strcmp( op, "RA" ) )  a.reset();
        else if( !strcmp( op, "FA" ) )  a.flip();
        else if( !strcmp( op, "RS" ) )  { a.resize( arg ); b.resize( arg ); }
        else if( !strcmp( op, "B" ) )   b.set( arg );
        else if( !strcmp( op, "BC" ) )  b.reset();
        else if( !strcmp( op, "AND" ) ) a &= b;
        else if( !strcmp( op, "OR" ) )  a |= b;
        else if( !strcmp( op, "XOR" ) ) a ^= b;
        else if( !strcmp( op, "NOT" ) ) a = ~a;
        else return 3;

        std::string bits;

        for( size_t i = 0; i < a.size(); ++i )
            bits += a.test( i ) ? '1' : '0';

        fprintf( out, "%zu %d %d %d %d %s\n", a.count(), a.any() ? 1 : 0, a.none() ? 1 : 0,
                 a.all() ? 1 : 0, a == b ? 1 : 0, bits.c_str() );
    }

    fclose( in );
    fclose( out );
    return 0;
}
