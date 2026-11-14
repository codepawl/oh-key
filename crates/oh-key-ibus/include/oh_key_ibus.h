// C interface to the oh-key IBus key handling. Strings are UTF-8 and not NUL terminated; those in an
// OhKeyIbusReaction are owned by the engine and valid until its next call.
#ifndef OH_KEY_IBUS_H
#define OH_KEY_IBUS_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct OhKeyIbusEngine OhKeyIbusEngine;

enum {
    OH_KEY_IBUS_PREEDIT_KEEP = 0,
    OH_KEY_IBUS_PREEDIT_SHOW = 1,
    OH_KEY_IBUS_PREEDIT_HIDE = 2,
};

typedef struct {
    uint8_t handled;
    uint8_t preedit_change;
    const uint8_t *commit;
    size_t commit_length;
    const uint8_t *preedit;
    size_t preedit_length;
} OhKeyIbusReaction;

OhKeyIbusEngine *oh_key_ibus_engine_new(void);
void oh_key_ibus_engine_free(OhKeyIbusEngine *engine);
void oh_key_ibus_engine_reload(OhKeyIbusEngine *engine);
void oh_key_ibus_engine_process_key(OhKeyIbusEngine *engine, uint32_t keysym, uint32_t state, OhKeyIbusReaction *out);
void oh_key_ibus_engine_abandon_word(OhKeyIbusEngine *engine, OhKeyIbusReaction *out);
uint8_t oh_key_ibus_engine_is_vietnamese(const OhKeyIbusEngine *engine);

#ifdef __cplusplus
}
#endif

#endif
