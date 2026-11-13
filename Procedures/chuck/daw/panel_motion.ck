// Shared by every panel: author's low morph (version 1).
1 => int version;
(me.args() > 0 ? Std.atof(me.arg(0)) : 1.0) => float dawSfxVolume;
(me.args() > 1 ? Std.atof(me.arg(1)) : 0.0) => float delayMilliseconds;
delayMilliseconds::ms => now;

// Workstation feedback bus preserves the engine's stereo output routing.
JCRev room => Gain volume => dac.chan(17);
0.24 => room.mix;
0.045 * dawSfxVolume => volume.gain;

fun void facet(float hz, float drift, float strength)
{
    SinOsc a => ADSR env => room;
    SinOsc b => env;
    SinOsc c => env;

    strength => a.gain;
    strength * 0.18 => b.gain;
    strength * 0.04 => c.gain;

    hz => a.freq;
    hz * 2.76 => b.freq;
    hz * 5.4 => c.freq;

    env.set(65::ms, 140::ms, 0.25, 240::ms);
    env.keyOn();

    for (0 => int i; i < 110; i++)
    {
        i / 109.0 => float t;
        t * t * (3.0 - 2.0 * t) => float bend;

        hz * (1.0 + drift * bend) => float f;
        f => a.freq;
        f * (2.76 - 0.035 * bend) => b.freq;
        f * (5.4 + 0.025 * bend) => c.freq;

        2::ms => now;
    }

    env.keyOff();
    260::ms => now;
}

fun void morph(float pitch)
{
    spork ~ facet(1480.0 * pitch,  0.025, 0.14);
    25::ms => now;
    spork ~ facet(2093.0 * pitch, -0.020, 0.09);
    35::ms => now;
    spork ~ facet(2347.0 * pitch, -0.035, 0.06);

    750::ms => now;
}

// 1: low; 2: original; 3: higher; 4: highest.
[0.75, 1.0, 1.25, 1.5] @=> float tones[];

if (version >= 1 && version <= 4)
{
    morph(tones[version - 1]);
}
