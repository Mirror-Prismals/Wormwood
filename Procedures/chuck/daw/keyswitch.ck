// Soft linear black-switch keycap: muted bottom-out, no click-jacket snap.
// Keep the existing feedback channel and caller-controlled DAW SFX volume.
Gain master => dac.chan(17);
Noise contact => LPF contactLPF => ADSR contactEnv => master;
SinOsc body => ADSR bodyEnv => master;
(me.args() > 0 ? Std.atof(me.arg(0)) : 1.0) => float dawSfxVolume;
0.22 * dawSfxVolume => master.gain;

// Rounded plastic contact, with the bright switch click filtered away.
0.35 => contact.gain;
1500.0 => contactLPF.freq;
0.7 => contactLPF.Q;
contactEnv.set(0.8::ms, 3::ms, 0.0, 6::ms);

// A brief, damped keycap body rather than the former piercing 10 kHz tone.
320.0 => body.freq;
0.24 => body.gain;
bodyEnv.set(1::ms, 9::ms, 0.0, 6::ms);

contactEnv.keyOn();
bodyEnv.keyOn();
12::ms => now;
contactEnv.keyOff();
bodyEnv.keyOff();
8::ms => now;
