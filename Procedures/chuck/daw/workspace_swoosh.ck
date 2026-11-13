// Directional workspace travel: a short, soft band-passed noise sweep.
Noise air => BPF band => LPF softener => Gain master => dac.chan(17);
(me.args() > 0 ? Std.atof(me.arg(0)) : 1.0) => float dawSfxVolume;
0.42 => band.Q;
7200.0 => softener.freq;
0.0 => master.gain;

190::ms => dur travel;
now => time started;
while (now - started < travel) {
    (now - started) / travel => float phase;
    Math.sin(Math.PI * phase) => float envelope;
    320.0 + 3300.0 * Math.pow(phase, 1.45) => band.freq;
    0.19 * dawSfxVolume * envelope * envelope => master.gain;
    2::ms => now;
}
0.0 => master.gain;
