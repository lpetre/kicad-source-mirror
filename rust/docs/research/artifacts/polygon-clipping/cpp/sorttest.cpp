// Reference for the std::sort port test: prints the permutation std::sort produces.
#include <algorithm>
#include <cstdio>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

struct Item { long long key; long long tag; };
struct Less { inline bool operator()( const Item& a, const Item& b ) { return a.key < b.key; } };

int main()
{
    std::string line;
    while( std::getline( std::cin, line ) )
    {
        std::istringstream ss( line );
        size_t n;
        if( !( ss >> n ) ) continue;
        std::vector<Item> v( n );
        for( size_t i = 0; i < n; i++ ) { ss >> v[i].key; v[i].tag = (long long) i; }
        std::sort( v.begin(), v.end(), ( Less() ) );
        std::string o;
        for( size_t i = 0; i < n; i++ ) { if( i ) o += ' '; o += std::to_string( v[i].tag ); }
        puts( o.c_str() );
    }
}
