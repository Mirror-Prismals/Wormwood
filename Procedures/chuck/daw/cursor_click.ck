// Minimalist ADSR Crystal Hit
ModalBar crystal => ADSR env => Gain master => dac.chan(17);
(me.args() > 0 ? Std.atof(me.arg(0)) : 1.0) => master.gain;

// Very precise ADSR for a sharp, clean hit
env.set(100::ms, 150::ms, 0.0, 100::ms);

5 => crystal.preset;
1.0 => crystal.gain;

fun void sharpCrystalHit() {
    Std.mtof(84) => crystal.freq;
    
    env.keyOn();
    1.0 => crystal.strike;
    
    // Short and sweet
    300::ms => now;
    
    env.keyOff();
}

sharpCrystalHit();
