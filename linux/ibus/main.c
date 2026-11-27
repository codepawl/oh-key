// The oh-key IBus engine: thin glue between libibus and the Rust key handling in
// crates/oh-key-ibus. It owns the IBus objects; every decision about what is typed is made in Rust.
//
// The word being typed is the preedit (underlined), the finished word is committed. Clients that
// do not offer preedit text get their keys untouched, so typing never loses letters.

#include <ibus.h>
#include <stdio.h>
#include <string.h>

#include "oh_key_ibus.h"

#define ENGINE_NAME "oh-key"
#define COMPONENT_NAME "org.freedesktop.IBus.OhKey"

#define OH_KEY_TYPE_ENGINE (oh_key_engine_get_type())
#define OH_KEY_ENGINE(object) (G_TYPE_CHECK_INSTANCE_CAST((object), OH_KEY_TYPE_ENGINE, OhKeyEngine))

typedef struct {
    IBusEngine parent;
    OhKeyIbusEngine *typing;
    gboolean client_has_preedit;
} OhKeyEngine;

typedef struct {
    IBusEngineClass parent;
} OhKeyEngineClass;

G_DEFINE_TYPE(OhKeyEngine, oh_key_engine, IBUS_TYPE_ENGINE)

static void apply_reaction(IBusEngine *engine, const OhKeyIbusReaction *reaction) {
    if (reaction->preedit_change == OH_KEY_IBUS_PREEDIT_HIDE) {
        ibus_engine_hide_preedit_text(engine);
    }
    if (reaction->commit_length > 0) {
        gchar *commit = g_strndup((const gchar *)reaction->commit, reaction->commit_length);
        IBusText *text = ibus_text_new_from_string(commit);
        ibus_engine_commit_text(engine, text);
        g_free(commit);
    }
    if (reaction->preedit_change == OH_KEY_IBUS_PREEDIT_SHOW) {
        gchar *word = g_strndup((const gchar *)reaction->preedit, reaction->preedit_length);
        IBusText *text = ibus_text_new_from_string(word);
        ibus_text_append_attribute(text, IBUS_ATTR_TYPE_UNDERLINE, IBUS_ATTR_UNDERLINE_SINGLE, 0, -1);
        guint cursor = (guint)g_utf8_strlen(word, -1);
        ibus_engine_update_preedit_text_with_mode(engine, text, cursor, TRUE, IBUS_ENGINE_PREEDIT_COMMIT);
        g_free(word);
    }
}

static gboolean oh_key_engine_process_key_event(IBusEngine *engine, guint keyval, guint keycode, guint state) {
    OhKeyEngine *self = OH_KEY_ENGINE(engine);
    (void)keycode;
    if (!self->client_has_preedit) {
        return FALSE;
    }
    OhKeyIbusReaction reaction;
    oh_key_ibus_engine_process_key(self->typing, keyval, state, &reaction);
    apply_reaction(engine, &reaction);
    return reaction.handled != 0;
}

static void abandon_word(OhKeyEngine *self) {
    OhKeyIbusReaction reaction;
    oh_key_ibus_engine_abandon_word(self->typing, &reaction);
    apply_reaction(IBUS_ENGINE(self), &reaction);
}

static void oh_key_engine_focus_in(IBusEngine *engine) {
    OhKeyEngine *self = OH_KEY_ENGINE(engine);
    oh_key_ibus_engine_reload(self->typing);
    abandon_word(self);
}

static void oh_key_engine_focus_out(IBusEngine *engine) {
    abandon_word(OH_KEY_ENGINE(engine));
}

static void oh_key_engine_reset(IBusEngine *engine) {
    abandon_word(OH_KEY_ENGINE(engine));
}

static void oh_key_engine_set_capabilities(IBusEngine *engine, guint capabilities) {
    OhKeyEngine *self = OH_KEY_ENGINE(engine);
    self->client_has_preedit = (capabilities & IBUS_CAP_PREEDIT_TEXT) != 0;
}

static void oh_key_engine_finalize(GObject *object) {
    OhKeyEngine *self = OH_KEY_ENGINE(object);
    oh_key_ibus_engine_free(self->typing);
    self->typing = NULL;
    G_OBJECT_CLASS(oh_key_engine_parent_class)->finalize(object);
}

static void oh_key_engine_class_init(OhKeyEngineClass *klass) {
    GObjectClass *object_class = G_OBJECT_CLASS(klass);
    IBusEngineClass *engine_class = IBUS_ENGINE_CLASS(klass);
    object_class->finalize = oh_key_engine_finalize;
    engine_class->process_key_event = oh_key_engine_process_key_event;
    engine_class->focus_in = oh_key_engine_focus_in;
    engine_class->focus_out = oh_key_engine_focus_out;
    engine_class->reset = oh_key_engine_reset;
    engine_class->set_capabilities = oh_key_engine_set_capabilities;
}

static void oh_key_engine_init(OhKeyEngine *self) {
    self->typing = oh_key_ibus_engine_new();
    // Until the client says otherwise, assume it can show preedit text.
    self->client_has_preedit = TRUE;
}

static void on_bus_disconnected(IBusBus *bus, gpointer user_data) {
    (void)bus;
    (void)user_data;
    ibus_quit();
}

static void register_component(IBusBus *bus) {
    IBusComponent *component = ibus_component_new(
        COMPONENT_NAME, "oh-key Vietnamese input", VERSION_STRING, "GPL-3.0-or-later", "codepawl",
        "https://github.com/anxuanng/oh-key", "", "oh-key");
    IBusEngineDesc *description = ibus_engine_desc_new(
        ENGINE_NAME, "oh-key", "Vietnamese typing (Telex, VNI)", "vi", "GPL-3.0-or-later", "codepawl",
        "", "us");
    ibus_component_add_engine(component, description);
    ibus_bus_register_component(bus, component);
}

int main(int argc, char **argv) {
    gboolean started_by_ibus = FALSE;
    for (int index = 1; index < argc; index++) {
        if (strcmp(argv[index], "--ibus") == 0) {
            started_by_ibus = TRUE;
        } else if (strcmp(argv[index], "--version") == 0) {
            printf("ibus-engine-oh-key %s\n", VERSION_STRING);
            return 0;
        }
    }

    ibus_init();
    IBusBus *bus = ibus_bus_new();
    g_signal_connect(bus, "disconnected", G_CALLBACK(on_bus_disconnected), NULL);

    IBusFactory *factory = ibus_factory_new(ibus_bus_get_connection(bus));
    ibus_factory_add_engine(factory, ENGINE_NAME, OH_KEY_TYPE_ENGINE);

    if (started_by_ibus) {
        ibus_bus_request_name(bus, COMPONENT_NAME, 0);
    } else {
        register_component(bus);
    }
    ibus_main();
    return 0;
}
