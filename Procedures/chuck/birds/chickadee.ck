12 => int outputChannel;
if (me.args() > 0) Std.atoi(me.arg(0)) => outputChannel;

SinOsc whistle => Gain voice => Gain master => dac.chan(outputChannel);
Noise air => BPF airColor => Gain airGain => master;
0.18 => master.gain;
0.025 => airGain.gain;
4200 => airColor.freq;
5 => airColor.Q;

fun void note(float fromHz, float toHz, dur length, float peak) {
    (length / 1::ms) $ int => int steps;
    for (0 => int i; i < steps; i++) {
        i / (steps * 1.0) => float t;
        fromHz + (toHz - fromHz) * t => whistle.freq;
        Math.sin(Math.PI * t) * peak => voice.gain;
        1::ms => now;
    }
    0 => voice.gain;
}

note(4050, 4400, 72::ms, 0.42);
28::ms => now;
note(3300, 3050, 92::ms, 0.34);
18::ms => now;
note(3180, 2920, 84::ms, 0.30);
0 => master.gain;
