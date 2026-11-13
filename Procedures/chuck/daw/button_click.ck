// Placeholder DAW button click.
TriOsc tone => LPF lp => Gain g => dac.chan(17);
(me.args() > 0 ? Std.atof(me.arg(0)) : 1.0) => float dawSfxVolume;
0.14 * dawSfxVolume => g.gain;
2200.0 => lp.freq;
0.9 => tone.gain;

[1180.0, 980.0] @=> float notes[];
for (0 => int i; i < notes.size(); i++) {
    notes[i] => tone.freq;
    12::ms => now;
}

0.0 => g.gain;
