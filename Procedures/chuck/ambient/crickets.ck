    // Summer night with cicada sounds
    10 => int outputChannel;
    if (me.args() > 0) {
        Std.atoi(me.arg(0)) => outputChannel;
    }
    if (outputChannel < 0) {
        10 => outputChannel;
    }
    Gain g => dac.chan(outputChannel);
    g.gain(0.5);
    fun void cicada() {
        SinOsc s => LPF f => JCRev r => g;
        s.freq(3000);
        0.01 => s.gain;
        80 => f.freq;
        0.1 => r.mix;
        while (true) {
            Math.random2f(0.01, 0.03) :: second => dur chirp_dur;
            s.freq(3000 + Math.random2f(-100, 100));
            Math.random2f(0.05, 0.2) => s.gain;
            chirp_dur => now;
        }
    }
    for (0 => int i; i < 10; i++) {
        spork ~ cicada();
        0.2::second => now;
    }
    Noise n => LPF f => JCRev r => g;
    1.1 => n.gain;
    500 => f.freq;
    0.2 => r.mix;
    while (true) {
        Math.random2f(0.005, 0.01) => n.gain;
        1::second => now;
    }
