// Research driver: KiCad's use of the bundled nanoflann (convert_shape_list_to_polygon.cpp):
//   KDTreeSingleIndexAdaptor< L2_Simple_Adaptor<double, ADAPTOR>, ADAPTOR, 2 >, default params,
//   radiusSearch( pt, eps * eps, matches ) followed by KiCad's canonical std::sort, and
//   knnSearch( pt, 8, indices, dists ).
//
// Input: <ncases>; per case: <n> <nq>; n lines "x y"; nq lines "x y eps"
// Output per case: "case i"; per query:
//   R <k> <idx...>      radius matches in KiCad's canonical order (dist, index)
//   U <k> <idx...>      radius matches exactly as nanoflann returned them (sorted by distance only)
//   K <found> <idx...>  knn result order
#include <nanoflann.hpp>

#include <algorithm>
#include <cstdio>
#include <utility>
#include <vector>

struct ADAPTOR
{
    std::vector<std::pair<int, int>> pts;

    size_t kdtree_get_point_count() const { return pts.size(); }

    double kdtree_get_pt( const size_t idx, const size_t dim ) const
    {
        return dim == 0 ? static_cast<double>( pts[idx].first ) : static_cast<double>( pts[idx].second );
    }

    template <class BBOX>
    bool kdtree_get_bbox( BBOX& ) const
    {
        return false;
    }
};

using KDTree = nanoflann::KDTreeSingleIndexAdaptor<nanoflann::L2_Simple_Adaptor<double, ADAPTOR>, ADAPTOR, 2>;

static bool closerEndpoint( const nanoflann::ResultItem<uint32_t, double>& aLeft,
                            const nanoflann::ResultItem<uint32_t, double>& aRight )
{
    if( aLeft.second != aRight.second )
        return aLeft.second < aRight.second;

    return aLeft.first < aRight.first;
}

int main( int argc, char** argv )
{
    if( argc < 3 )
        return 2;

    FILE*  in = fopen( argv[1], "r" );
    FILE*  out = fopen( argv[2], "w" );
    size_t ncases = 0;

    if( !in || !out || fscanf( in, "%zu", &ncases ) != 1 )
        return 2;

    for( size_t c = 0; c < ncases; ++c )
    {
        size_t n = 0, nq = 0;

        if( fscanf( in, "%zu %zu", &n, &nq ) != 2 )
            return 2;

        ADAPTOR adaptor;
        adaptor.pts.resize( n );

        for( auto& p : adaptor.pts )
        {
            if( fscanf( in, "%d %d", &p.first, &p.second ) != 2 )
                return 2;
        }

        KDTree kdTree( 2, adaptor );

        fprintf( out, "case %zu\n", c );

        for( size_t q = 0; q < nq; ++q )
        {
            int    x = 0, y = 0;
            double eps = 0;

            if( fscanf( in, "%d %d %lf", &x, &y, &eps ) != 3 )
                return 2;

            const double query_pt[2] = { static_cast<double>( x ), static_cast<double>( y ) };
            const double radius_sq = eps * eps;

            std::vector<nanoflann::ResultItem<uint32_t, double>> matches;
            kdTree.radiusSearch( query_pt, radius_sq, matches );

            std::vector<nanoflann::ResultItem<uint32_t, double>> raw = matches;
            std::sort( matches.begin(), matches.end(), closerEndpoint );

            fprintf( out, "R %zu", matches.size() );

            for( const auto& m : matches )
                fprintf( out, " %u", m.first );

            fprintf( out, "\nU %zu", raw.size() );

            for( const auto& m : raw )
                fprintf( out, " %u", m.first );

            uint32_t indices[8] = { 0 };
            double   dists[8];
            const size_t found = kdTree.knnSearch( query_pt, 8, indices, dists );

            fprintf( out, "\nK %zu", found );

            for( size_t i = 0; i < found; ++i )
                fprintf( out, " %u", indices[i] );

            fprintf( out, "\n" );
        }
    }

    fclose( in );
    fclose( out );
    return 0;
}
