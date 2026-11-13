Gain master => dac.chan(17);
SinOsc s => BPF f => Gain g => master;
SqrOsc h => Gain hg => g;
(me.args() > 0 ? Std.atof(me.arg(0)) : 1.0) => master.gain;
0.1 => hg.gain;
3000 => h.freq;

7 => f.Q;
3000 => f.freq;

0.8 => g.gain;

// vibrato oscillator
SinOsc vibr => blackhole;
40 => vibr.freq;
1.0 => vibr.gain;
20 => float vibrDepth;   // vibrato depth in Hz!

// envelope shape
0.1 => float maxAmp;
0.0 => float amp;

5::ms => dur atk;
30::ms => dur sus;
60::ms => dur rel;

// pitch sweep params
2500 => float startFreq;
3100 => float peakFreq;

// attack
for (0 => int i; i < atk/1::ms; i++) {
    amp + (maxAmp - amp)*0.1 => amp;
    amp => g.gain;
    1::ms => now;
}

// upsweep + vibrato
(sus/1::ms) $ int => int steps;
for (0 => int i; i < steps; i++) {
    startFreq + (peakFreq - startFreq)*(i/(steps*1.0)) => float curFreq;
    // vibrato signal scaled by depth
    curFreq + vibrDepth * vibr.last() => float freqwob;
    freqwob => s.freq;
    freqwob => h.freq;
    curFreq => f.freq;      // filter follows central freq
    1::ms => now;
}

// decay
(rel/1::ms) $ int => int rsteps;
for (0 => int i; i < rsteps; i++) {
    amp * 0.7 => amp;
    amp => g.gain;
    1::ms => now;
}

0 => g.gain;
