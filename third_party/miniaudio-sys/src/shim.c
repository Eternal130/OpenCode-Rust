/* Vendored from opentui's native shim
 * (third_party/opentui/packages/native/src/miniaudio_shim.c, MIT) so this
 * crate compiles the exact same miniaudio configuration v2's TUI audio
 * engine uses: on Linux, only the ALSA + PulseAudio backends, discovered
 * via runtime dlopen (MA_NO_RUNTIME_LINKING is NOT defined here — the null
 * backend stays disabled, so a machine with no ALSA/Pulse libraries fails
 * device/context init and the caller degrades to a silent no-op, exactly
 * like v2's `Audio.create` → catch → null path).
 */
#define MINIAUDIO_IMPLEMENTATION

#if defined(__linux__)
#define MA_ENABLE_ONLY_SPECIFIC_BACKENDS
#define MA_ENABLE_ALSA
#define MA_ENABLE_PULSEAUDIO
#endif

#if defined(__APPLE__)
#define MA_ENABLE_ONLY_SPECIFIC_BACKENDS
#define MA_ENABLE_COREAUDIO
#endif

#if defined(_WIN32)
#define MA_ENABLE_ONLY_SPECIFIC_BACKENDS
#define MA_ENABLE_WASAPI
#define MA_ENABLE_DSOUND
#define MA_ENABLE_WINMM
#endif

#include "miniaudio/miniaudio.h"
