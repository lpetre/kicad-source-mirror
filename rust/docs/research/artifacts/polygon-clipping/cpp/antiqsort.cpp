// McIlroy's "A Killer Adversary for Quicksort" against std::sort: prints key sequences that
// drive libstdc++'s introsort into its heapsort fallback.  Output: "n k0 k1 ..." lines.
#include <algorithm>
#include <cstdio>
#include <string>
#include <vector>

static std::vector<int> val;
static int nsolid, candidate, gas;

struct Cmp
{
    bool operator()( int x, int y ) const
    {
        if( val[x] == gas && val[y] == gas )
        {
            if( x == candidate ) val[x] = nsolid++;
            else                 val[y] = nsolid++;
        }
        if( val[x] == gas )      candidate = x;
        else if( val[y] == gas ) candidate = y;
        return val[x] < val[y];
    }
};

int main()
{
    for( int n : { 17, 33, 64, 100, 257, 1000, 4096, 10000 } )
    {
        val.assign( n, 0 );
        std::vector<int> ptr( n );
        gas = n - 1;
        nsolid = candidate = 0;
        for( int i = 0; i < n; i++ ) { ptr[i] = i; val[i] = gas; }
        std::sort( ptr.begin(), ptr.end(), Cmp() );
        for( int div : { 1, 2, 3, 7 } )
        {
            std::string o = std::to_string( n );
            for( int i = 0; i < n; i++ ) { o += ' '; o += std::to_string( val[i] / div ); }
            puts( o.c_str() );
        }
    }
}
