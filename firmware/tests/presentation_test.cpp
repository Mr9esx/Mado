#include "../main/presentation.h"

#include <cassert>
#include <limits>

int main() {
    using namespace presentation;
    assert(blend_coverage(255, 0, 0) == 255);
    assert(blend_coverage(255, 0, 128) == 128);
    assert(blend_coverage(0, 255, 128) == 128);
    assert(blend_coverage(0, 255, 255) == 254);
    assert(compact_metric(0) == "0");
    assert(compact_metric(1.25) == "1.25");
    assert(compact_metric(0.125) == "0.13");
    assert(compact_metric(2.675) == "2.67");
    assert(compact_metric(1234) == "1.2k");
    assert(compact_metric(-1234) == "-1k");
    assert(compact_metric(123456) == "120k");
    assert(compact_metric(999999) == "1m");
    assert(compact_metric(1e20) == "1e20");
    assert(compact_metric(8.5e16) == "9e16");
    assert(compact_metric(-1e20) == "超限");
    assert(compact_metric(std::numeric_limits<double>::quiet_NaN()) == "—");

    assert(count_duration(80, 90) == 400);
    assert(count_duration(1, 2) == 300);
    assert(count_duration(0, 100) == 800);
    assert(counted_value(80, 90, 40) == 81);
    assert(counted_value(12.3, 12.5, 150) == 12.4);

    assert(roll_distance(9, 0, 1) == 1);
    assert(roll_distance(0, 9, -1) == 1);
    assert(roll_distance(2, 4, 1) == 2);
    assert(roll_digit(2, 1, 1) == '3');
    assert(roll_digit(0, 2, -1) == '8');
}
