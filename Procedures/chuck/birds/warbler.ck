12 => int outputChannel;
if (me.args() > 0) Std.atoi(me.arg(0)) => outputChannel;

SinOsc tone => Gain voice => Gain master => dac.chan(outputChannel);
SinOsc shimmer => Gain shimmerGain => master;
0.15 => master.gain;
0.0 => voice.gain;
0.0 => shimmerGain.gain;

fun void chirp(float lowHz, float highHz, dur length, float peak) {
    (length / 1::ms) $ int => int steps;
    for (0 => int i; i < steps; i++) {
        i / (steps * 1.0) => float t;
        Math.sin(Math.PI * t) => float envelope;
        lowHz + (highHz - lowHz) * Math.sin(t * Math.PI * 0.72) => float hz;
        hz => tone.freq;
        hz * 1.985 => shimmer.freq;
        envelope * peak => voice.gain;
        envelope * peak * 0.10 => shimmerGain.gain;
        1::ms => now;
    }
    0 => voice.gain;
    0 => shimmerGain.gain;
}

for (0 => int i; i < 5; i++) {
    chirp(2850 + i * 95, 4750 + i * 55, 48::ms, 0.34 - i * 0.025);
    (16 + i * 3)::ms => now;
}
chirp(3900, 2450, 105::ms, 0.28);
0 => master.gain;
