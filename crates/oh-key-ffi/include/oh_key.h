// C interface to the oh-key typing engine. Strings are UTF-8 and not NUL terminated; those in an
// OhKeyResult are owned by the session and valid until its next call.
#ifndef OH_KEY_H
#define OH_KEY_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct OhKeySession OhKeySession;

typedef struct {
    uint32_t backspaces;
    const uint8_t *text;
    size_t text_length;
    uint8_t consume;
    uint8_t composing;
    const uint8_t *preedit;
    size_t preedit_length;
    const uint8_t *finished_text;
    size_t finished_text_length;
} OhKeyResult;

enum {
    OH_KEY_INPUT_TELEX = 0,
    OH_KEY_INPUT_VNI = 1,
    OH_KEY_INPUT_VIQR = 2,
    OH_KEY_INPUT_SIMPLE_TELEX = 3,
};

enum {
    OH_KEY_HOTKEY_CTRL_SHIFT = 0,
    OH_KEY_HOTKEY_ALT_Z = 1,
    OH_KEY_HOTKEY_DISABLED = 2,
};

OhKeySession *oh_key_session_new(void);
void oh_key_session_free(OhKeySession *session);
void oh_key_session_load_settings(OhKeySession *session, const char *text, size_t length);
void oh_key_session_load_macros(OhKeySession *session, const char *text, size_t length);
void oh_key_session_settings_text(OhKeySession *session, const uint8_t **text, size_t *length);
void oh_key_session_set_input_method(OhKeySession *session, uint32_t code);
uint32_t oh_key_session_input_method(const OhKeySession *session);
uint8_t oh_key_session_is_app_excluded(const OhKeySession *session, const char *name, size_t length);
uint8_t oh_key_session_starts_vietnamese(const OhKeySession *session);
uint32_t oh_key_session_toggle_hotkey(const OhKeySession *session);
void oh_key_session_process_key(OhKeySession *session, uint32_t character, OhKeyResult *result);
void oh_key_session_backspace(OhKeySession *session, OhKeyResult *result);
void oh_key_session_finish_word(OhKeySession *session, OhKeyResult *result);
void oh_key_session_reset(OhKeySession *session);

#ifdef __cplusplus
}
#endif

#endif
