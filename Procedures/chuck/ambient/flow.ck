// Moving soft-body water layer.
// C++ supplies the dedicated head-speaker bus channel as argv[0] and
// controls audibility from nearby particle density and velocity.

8 => int outputChannel;
if (me.args() > 0) {
    Std.atoi(me.arg(0)) => outputChannel;
}
if (outputChannel < 0) {
    8 => outputChannel;
}

Gain master => dac.chan(outputChannel);
0.5 => master.gain;

Noise water => LPF filter => master;
2000.0 => filter.freq;
0.2 => filter.Q;
0.1 => water.gain;

while (true) {
    100::ms => now;
}
