/* C-level test of the chromagrain ABI: build a source from raw PCM + RGBA,
 * play notes through the macros/key/transport, check the audio and the
 * grain snapshot. Exit code 0 = pass. Built and run by tests/c_api.rs. */

#include "chromagrain.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define SR 48000
#define BLOCK 256

static int failures = 0;
#define CHECK(cond, ...)                                  \
    do {                                                  \
        if (!(cond)) {                                    \
            fprintf(stderr, "FAIL %s:%d: ", __FILE__, __LINE__); \
            fprintf(stderr, __VA_ARGS__);                 \
            fprintf(stderr, "\n");                        \
            failures++;                                   \
        }                                                 \
    } while (0)

static double rms(const float *x, size_t n) {
    double s = 0;
    for (size_t i = 0; i < n; i++) s += (double)x[i] * x[i];
    return n ? sqrt(s / n) : 0;
}

/* Render `frames` in BLOCK-sized calls into l/r. */
static void run(CgInstrument *inst, float *l, float *r, size_t frames) {
    for (size_t pos = 0; pos < frames; pos += BLOCK) {
        size_t n = frames - pos < BLOCK ? frames - pos : BLOCK;
        cg_instrument_process(inst, l + pos, r + pos, n);
    }
}

int main(void) {
    CgInstrument *inst = cg_instrument_new(SR);
    CHECK(inst != NULL, "cg_instrument_new returned NULL");
    CHECK(cg_instrument_sample_rate(inst) == SR, "sample rate");
    CHECK(cg_instrument_active_source(inst) == -1, "no source yet");

    /* 2 s of interleaved stereo 220 Hz sine + 24 frames of 16x8 RGBA at 12 fps. */
    size_t n = 2 * SR;
    float *pcm = malloc(sizeof(float) * n * 2);
    for (size_t i = 0; i < n; i++) {
        float s = 0.5f * sinf(2.0f * (float)M_PI * 220.0f * (float)i / SR);
        pcm[2 * i] = s;
        pcm[2 * i + 1] = -s;
    }
    size_t frame_bytes = 16 * 8 * 4, n_frames = 24;
    unsigned char *rgba = malloc(frame_bytes * n_frames);
    for (size_t i = 0; i < frame_bytes * n_frames; i++) rgba[i] = (unsigned char)(i * 7);

    int id = cg_instrument_add_source_pcm_rgba(inst, "sine", pcm, n * 2, 2, SR, rgba, frame_bytes * n_frames, 16, 8, 12.0f, 220.0f);
    free(pcm);
    free(rgba);
    CHECK(id == 0, "first source id is 0 (got %d)", id);
    CHECK(cg_instrument_source_count(inst) == 1, "one source");
    CHECK(cg_instrument_active_source(inst) == 0, "first source is active");
    CHECK(fabs(cg_instrument_source_duration(inst, 0) - 2.0) < 0.01, "duration %f", cg_instrument_source_duration(inst, 0));
    CHECK(!cg_instrument_select_source(inst, 5), "unknown id rejected");

    /* Controls. */
    cg_instrument_set_key(inst, 9, CG_SCALE_MINOR);
    CHECK(cg_instrument_key_root(inst) == 9 && cg_instrument_key_scale(inst) == CG_SCALE_MINOR, "key A minor");
    cg_instrument_set_macro(inst, CG_MACRO_GRAIN, 0.6f);
    cg_instrument_set_macro(inst, CG_MACRO_SMEAR, 0.2f);
    CHECK(fabsf(cg_instrument_macro_value(inst, CG_MACRO_GRAIN) - 0.6f) < 1e-6f, "grain macro");
    CHECK(fabsf(cg_instrument_macro_value(inst, CG_MACRO_PITCH) - 0.5f) < 1e-6f, "pitch defaults to centre");
    cg_instrument_set_macro(inst, 99, 1.0f); /* ignored */
    cg_instrument_set_sync(inst, 0.25f);
    CgTransport t = {120.0, true, 4.0};
    cg_instrument_set_transport(inst, t);
    CHECK(cg_instrument_latency_samples(inst) == 0, "no look, no latency");

    /* Play. */
    size_t frames = SR; /* 1 s */
    float *l = calloc(frames, sizeof(float)), *r = calloc(frames, sizeof(float));
    run(inst, l, r, frames / 10); /* silence before any note */
    CHECK(rms(l, frames / 10) == 0.0, "silent before notes");
    cg_instrument_note_on(inst, 57, 0.9f);
    cg_instrument_note_on(inst, 64, 0.7f);
    CHECK(cg_instrument_held_note_count(inst) == 2, "two notes held");
    run(inst, l, r, frames);
    int finite = 1;
    for (size_t i = 0; i < frames; i++)
        if (!isfinite(l[i]) || !isfinite(r[i]) || fabsf(l[i]) > 1.0f || fabsf(r[i]) > 1.0f) finite = 0;
    CHECK(finite, "audio finite and within +-1");
    CHECK(rms(l + SR / 10, frames - SR / 10) > 0.02, "notes are audible (rms %f)", rms(l + SR / 10, frames - SR / 10));

    CgGrain grains[64];
    double now = cg_instrument_now_secs(inst);
    CHECK(fabs(now - 1.1) < 1e-6, "clock advanced (%f)", now);
    size_t visible = cg_instrument_visible_grains(inst, now, grains, 64);
    CHECK(visible > 0, "grains on screen");
    for (size_t i = 0; i < visible && i < 64; i++) {
        CHECK(grains[i].source == 0, "grain source");
        CHECK(grains[i].onset <= now && now < grains[i].onset + grains[i].duration, "grain is visible now");
        CHECK(grains[i].pan >= -1.0f && grains[i].pan <= 1.0f, "pan in range");
    }
    CHECK(cg_instrument_visible_grains(inst, now, NULL, 0) == visible, "count-only query");
    CgTransport now_t = cg_instrument_transport(inst);
    CHECK(fabs(now_t.beat - (4.0 + 1.1 * 2.0)) < 1e-6, "beat advanced while playing (%f)", now_t.beat);

    /* Release rings out to silence. */
    cg_instrument_all_notes_off(inst);
    CHECK(cg_instrument_held_note_count(inst) == 0, "all notes off");
    cg_instrument_set_macro(inst, CG_MACRO_SMEAR, 0.0f);
    run(inst, l, r, frames);
    CHECK(rms(l + frames / 2, frames / 2) < 1e-4, "silent after release (%f)", rms(l + frames / 2, frames / 2));

    /* Reset keeps the media, rewinds the clock. */
    cg_instrument_reset(inst, 44100);
    CHECK(cg_instrument_sample_rate(inst) == 44100 && cg_instrument_now_secs(inst) == 0.0, "reset");
    CHECK(cg_instrument_source_count(inst) == 1, "media survives reset");

    /* NULL handles are tolerated. */
    cg_instrument_note_on(NULL, 60, 1.0f);
    cg_instrument_process(NULL, l, r, 16);
    CHECK(l[0] == 0.0f, "NULL process writes silence");
    cg_instrument_free(NULL);

    cg_instrument_free(inst);
    free(l);
    free(r);
    if (failures) {
        fprintf(stderr, "%d failure(s)\n", failures);
        return 1;
    }
    printf("chromagrain C ABI: all checks passed\n");
    return 0;
}
