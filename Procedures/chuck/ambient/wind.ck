/*
  plains_ambient_main_loop.ck
  Soothing outdoor daytime ambience for a plains biome.

  Layers:
  - broad wind bed (filtered noise)
  - occasional breeze gusts
  - soft grass/leaves rustle
  - sparse bird chirps
  - very subtle warm air tone

  Run:
    chuck plains_ambient_main_loop.ck
*/

// ----- player-head bus -----
2 => int outputChannel;
if (me.args() > 0) Std.atoi(me.arg(0)) => outputChannel;
Gain MASTER => LPF MASTER_TONE => JCRev REV => dac.chan(outputChannel);
0.0 => MASTER.gain;
6800 => MASTER_TONE.freq;
0.17 => REV.mix;

0.28 => float MASTER_TARGET_GAIN;

// ----- helpers -----
fun float lerp(float a, float b, float t)
{
    return a + ((b - a) * t);
}

fun float smooth01(float t)
{
    // smoothstep: keeps transitions soft
    return t * t * (3.0 - (2.0 * t));
}

fun float triangle01(float t)
{
    if (t < 0.5) return 2.0 * t;
    return 2.0 * (1.0 - t);
}

// ----- wind bed -----
fun void windBed()
{
    Noise n => LPF lpf => HPF hpf => Gain g => MASTER;
    1500 => lpf.freq;
    90 => hpf.freq;
    0.05 => g.gain;

    while (true)
    {
        g.gain() => float startGain;
        lpf.freq() => float startCutoff;

        Std.rand2f(0.03, 0.09) => float targetGain;
        Std.rand2f(900.0, 2600.0) => float targetCutoff;
        Std.rand2f(4.0, 11.0)::second => dur segment;

        80 => int steps;
        for (0 => int i; i < steps; i++)
        {
            smooth01((i $ float) / ((steps - 1) $ float)) => float t;
            lerp(startGain, targetGain, t) => g.gain;
            lerp(startCutoff, targetCutoff, t) => lpf.freq;
            segment / steps => now;
        }
    }
}

// ----- mid-sized breeze gusts -----
fun void breezeGusts()
{
    Noise n => BPF bpf => Gain g => MASTER;
    700 => bpf.freq;
    1.2 => bpf.Q;
    0.0 => g.gain;

    while (true)
    {
        Std.rand2f(3.0, 10.0)::second => now;

        Std.rand2f(320.0, 1200.0) => bpf.freq;
        Std.rand2f(0.6, 2.0) => bpf.Q;
        Std.rand2f(0.012, 0.045) => float peak;

        Std.rand2f(0.6, 1.8)::second => dur rise;
        Std.rand2f(1.0, 3.5)::second => dur fall;

        40 => int riseSteps;
        for (0 => int i; i < riseSteps; i++)
        {
            smooth01((i $ float) / ((riseSteps - 1) $ float)) => float t;
            lerp(0.0, peak, t) => g.gain;
            rise / riseSteps => now;
        }

        80 => int fallSteps;
        for (0 => int i; i < fallSteps; i++)
        {
            smooth01((i $ float) / ((fallSteps - 1) $ float)) => float t;
            lerp(peak, 0.0, t) => g.gain;
            fall / fallSteps => now;
        }
    }
}

// ----- grass / leaf texture -----
fun void grassRustle()
{
    Noise n => BPF bpf => Gain g => MASTER;
    1400 => bpf.freq;
    4.0 => bpf.Q;
    0.0 => g.gain;

    while (true)
    {
        Std.rand2f(0.4, 2.4)::second => now;

        Std.rand2f(800.0, 2800.0) => bpf.freq;
        Std.rand2f(2.0, 8.0) => bpf.Q;
        Std.rand2f(0.003, 0.018) => float peak;

        Std.rand2f(40.0, 120.0)::ms => dur attack;
        Std.rand2f(90.0, 260.0)::ms => dur release;

        14 => int aSteps;
        for (0 => int i; i < aSteps; i++)
        {
            smooth01((i $ float) / ((aSteps - 1) $ float)) => float t;
            lerp(0.0, peak, t) => g.gain;
            attack / aSteps => now;
        }

        18 => int rSteps;
        for (0 => int i; i < rSteps; i++)
        {
            smooth01((i $ float) / ((rSteps - 1) $ float)) => float t;
            lerp(peak, 0.0, t) => g.gain;
            release / rSteps => now;
        }
    }
}

// ----- single bird chirp voice -----
fun void birdChirp(float startFreq, float endFreq, dur length, float amp)
{
    SinOsc s => LPF lpf => Gain g => MASTER;
    0.0 => g.gain;
    0.8 => s.gain;
    5000 => lpf.freq;

    36 => int steps;
    for (0 => int i; i < steps; i++)
    {
        (i $ float) / ((steps - 1) $ float) => float t;
        triangle01(t) => float env;
        lerp(startFreq, endFreq, smooth01(t)) => s.freq;
        amp * env => g.gain;
        length / steps => now;
    }
}

// ----- sparse bird phrases -----
fun void birds()
{
    while (true)
    {
        Std.rand2f(6.0, 18.0)::second => now;
        Std.rand2(1, 3) => int chirpCount;

        for (0 => int i; i < chirpCount; i++)
        {
            Std.rand2f(1400.0, 2400.0) => float f0;
            Std.rand2f(2200.0, 3600.0) => float f1;
            Std.rand2f(90.0, 220.0)::ms => dur d;
            Std.rand2f(0.010, 0.028) => float a;

            spork ~ birdChirp(f0, f1, d, a);
            Std.rand2f(120.0, 360.0)::ms => now;
        }
    }
}

// ----- warm "summer air" tonal bed -----
fun void warmAirTone()
{
    SinOsc s => LPF lpf => Gain g => MASTER;
    0.0 => g.gain;
    220.0 => s.freq;
    650.0 => lpf.freq;

    while (true)
    {
        g.gain() => float g0;
        s.freq() => float f0;

        Std.rand2f(0.0015, 0.0050) => float g1;
        Std.rand2f(180.0, 320.0) => float f1;
        Std.rand2f(8.0, 20.0)::second => dur seg;

        80 => int steps;
        for (0 => int i; i < steps; i++)
        {
            smooth01((i $ float) / ((steps - 1) $ float)) => float t;
            lerp(g0, g1, t) => g.gain;
            lerp(f0, f1, t) => s.freq;
            seg / steps => now;
        }
    }
}

// ----- launch -----
spork ~ windBed();
spork ~ breezeGusts();
spork ~ grassRustle();
// spork ~ birds();
// spork ~ warmAirTone();

// Fade in so the loop starts naturally.
12::second => dur fadeIn;
160 => int fadeSteps;
for (0 => int i; i < fadeSteps; i++)
{
    smooth01((i $ float) / ((fadeSteps - 1) $ float)) => float t;
    lerp(0.0, MASTER_TARGET_GAIN, t) => MASTER.gain;
    fadeIn / fadeSteps => now;
}

// Keep running forever.
while (true) 1::minute => now;
