// Level 4: The Trench (Complex)
// Heavy spatial pressure and "whale-like" songs with frequency glides.

// Cardinal passes this moving speaker's dedicated output channel as arg 0.
11 => int outputChannel;
if (me.args() > 0) {
    Std.atoi(me.arg(0)) => outputChannel;
}
if (outputChannel < 0) {
    11 => outputChannel;
}

// --- Global Effects ---
NRev reverb => dac.chan(outputChannel);
0.25 => reverb.mix;

// --- Layer 1: Heavy Pressure (Sub-bass rumble) ---
Noise pressure => LPF pressureFilter => Pan2 pressurePan => reverb;
80.0 => pressureFilter.freq;
0.2 => pressure.gain;

// --- Layer 2: Whale Song (Gliding FM) ---
SinOsc mod => SinOsc car => ADSR whaleEnv => Pan2 whalePan => reverb;
2 => car.sync; // FM Sync
0.1 => car.gain;
(4::second, 1::second, 0.7, 4::second) => whaleEnv.set;

// --- Layer 3: Glowing Pulsars (Stereo) ---
SinOsc pulsars[3];
ADSR pulsarEnvs[3];
Pan2 pulsarPans[3];
[110.0, 164.8, 220.0] @=> float pulsarFreqs[];

for( 0 => int i; i < 3; i++ ) {
    pulsars[i] => pulsarEnvs[i] => pulsarPans[i] => reverb;
    pulsarFreqs[i] => pulsars[i].freq;
    0.03 => pulsars[i].gain;
    pulsarEnvs[i].set( 3::second, 0::second, 1.0, 3::second );
    Math.random2f(-1.0, 1.0) => pulsarPans[i].pan;
}

// --- Whale Song Trigger Thread ---
fun void whaleSong() {
    while(true) {
        // Random pitch in the deep bass
        Math.random2f(40, 100) => float targetFreq;
        targetFreq => car.freq;
        car.freq() * 0.5 => mod.freq;
        Math.random2f(10, 50) => mod.gain; // FM depth
        
        Math.random2f(-0.8, 0.8) => whalePan.pan;
        1 => whaleEnv.keyOn;
        
        // Glide the frequency during the note
        now => time start;
        start + 5::second => time end;
        while( now < end ) {
            car.freq() + Math.random2f(-0.5, 0.5) => car.freq;
            20::ms => now;
        }
        
        1 => whaleEnv.keyOff;
        Math.random2f(5, 15)::second => now; // Distant calls
    }
}

// --- Main Execution ---
spork ~ whaleSong();

// Pulsar Loop
while(true) {
    for(0 => int i; i < 3; i++) {
        spork ~ triggerPulsar(i);
        Math.random2f(1, 2)::second => now;
    }
    10::second => now;
}

fun void triggerPulsar(int idx) {
    1 => pulsarEnvs[idx].keyOn;
    4::second => now;
    1 => pulsarEnvs[idx].keyOff;
}
