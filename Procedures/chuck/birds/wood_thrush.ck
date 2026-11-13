12 => int outputChannel;
if (me.args() > 0) Std.atoi(me.arg(0)) => outputChannel;

SinOsc flute => Gain fluteGain => Gain master => dac.chan(outputChannel);
SinOsc overtone => Gain overtoneGain => master;
0.16 => master.gain;
0 => fluteGain.gain;
0 => overtoneGain.gain;

fun void phrase(float rootHz, float destinationHz, dur length) {
    (length / 1::ms) $ int => int steps;
    for (0 => int i; i < steps; i++) {
        i / (steps * 1.0) => float t;
        0.5 - 0.5 * Math.cos(2.0 * Math.PI * t) => float envelope;
        rootHz + (destinationHz - rootHz) * t
            + Math.sin(t * Math.PI * 7.0) * 18.0 => float hz;
        hz => flute.freq;
        hz * 2.01 => overtone.freq;
        envelope * 0.36 => fluteGain.gain;
        envelope * 0.055 => overtoneGain.gain;
        1::ms => now;
    }
    0 => fluteGain.gain;
    0 => overtoneGain.gain;
}

phrase(2050, 2580, 145::ms);
42::ms => now;
phrase(2450, 2180, 125::ms);
55::ms => now;
phrase(2220, 2920, 170::ms);
0 => master.gain;
