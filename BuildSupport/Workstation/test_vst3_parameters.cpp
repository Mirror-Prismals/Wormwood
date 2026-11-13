#include "Host/ParameterMailbox.h"
#include <cassert>
#include <limits>
#include <thread>
#include <cstdio>

int main() {
    NormalizedParameterMailbox values;
    values.configure({2,17,9001});
    // A bypassed insert can accumulate far more than 256 editor gestures. The
    // next block must still receive the final value.
    for(int i=0;i<10000;++i)assert(values.publish(17,(i%101)/100.));
    assert(values.publish(17,.731));
    int updates=0;
    values.consume([&](uint32_t id,double v){assert(id==17&&v==.731);++updates;});
    assert(updates==1);
    values.consume([](uint32_t,double){assert(false);});
    assert(values.publish(17,.812));
    values.configure({2,17,9001});
    values.configure({9001,17,2,4});
    values.consume([](uint32_t id,double v){assert(id==17&&v==.812);});
    assert(!values.publish(500,0.));
    assert(!values.publish(17,std::numeric_limits<double>::quiet_NaN()));
    assert(!values.publish(17,std::numeric_limits<double>::infinity()));
    assert(!values.publish(17,-.1));
    assert(values.publish(2,-0.));
    values.consume([](uint32_t id,double v){assert(id==2&&v==0.&&!std::signbit(v));});
    std::atomic<bool> done{false};
    std::thread editor([&]{
        for(int i=0;i<100000;++i)assert(values.publish(9001,(i%101)/100.));
        assert(values.publish(9001,.923));
        done.store(true,std::memory_order_release);
    });
    double last=-1;
    auto consume=[&](uint32_t id,double v){assert(id==9001&&std::isfinite(v)&&v>=0&&v<=1);last=v;};
    while(!done.load(std::memory_order_acquire))values.consume(consume);
    editor.join();values.consume(consume);assert(last==.923);
    std::puts("PASS: bounded VST3 parameter coalescing, invalid-value rejection and concurrent final-value delivery");
}
