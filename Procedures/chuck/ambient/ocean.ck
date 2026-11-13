    // Ocean waves sound generator
    9 => int outputChannel;
    if (me.args() > 0) {
        Std.atoi(me.arg(0)) => outputChannel;
    }
    Gain g => dac.chan(outputChannel);
    Noise n => LPF f => g;
    f.freq(500.0);
    f.Q(1.0);
    SinOsc mod1 => blackhole;
    SinOsc mod2 => blackhole;
    mod1.freq(0.2);
    mod2.freq(0.1);
    g.gain(0.5);
    fun float mod_amplitude() {
        return 0.5 + 0.4 * mod1.last() + 0.2 * mod2.last();
    }
    while (true) {
        n.gain(mod_amplitude());
        1::ms => now;
    }
