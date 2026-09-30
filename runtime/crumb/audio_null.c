#include "crumb.h"
#include "crumb_internal.h"

/* PPM and browser development never open an audio device on the game host. */
void crumb_audio_init(void) {}
void crumb_audio_shutdown(void) {}
void crumb_tone(float frequency, float seconds, float volume) {
    (void)frequency;
    (void)seconds;
    (void)volume;
}
void crumb_noise(float seconds, float volume) {
    (void)seconds;
    (void)volume;
}
void crumb_sound_play(int sound, float volume) {
    (void)sound;
    (void)volume;
}
void crumb_sound_pause(void) {}
void crumb_sound_resume(void) {}
void crumb_sound_stop(void) {}
float crumb_sound_position(void) { return 0.0f; }
void crumb_sound_seek(float seconds) { (void)seconds; }
