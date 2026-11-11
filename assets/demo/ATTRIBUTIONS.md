# Demo Media Attributions (`assets/demo/`)

All demo media in `assets/demo/` is public-domain archival footage paired with original generated or procedurally synthesized audio (zero copyrighted music or commercial samples). Total size: ~7.5 MB.

## 1. Archival Public-Domain / Open-License Video Sources
Trimmed and graded from the local `vibecut/videos/` archival collection:

| Demo File | Archival Video Source | Trim Window | Visual Content |
|---|---|---|---|
| `rain_window.mp4` | `rainy_window.mp4` (CC0 / Public Domain stock) | `00:00:08.0` – `00:00:20.0` | Raindrops sliding down dark windowpane against warm sodium/crimson streetlight bokeh |
| `night_chords.mp4` | `new_york_world_s_fair_part_3.mp4` (Prelinger Archives, Public Domain) | `00:53:44.0` – `00:53:56.0` | Nocturnal amber and cyan fountain reflections on black night sky |
| `sub_highway.mp4` | `calvin_0302.mp4` (Calvin Productions / Prelinger Archives, Public Domain) | `00:02:53.8` – `00:02:57.8` | Oncoming car headlights streaking across wet dark highway asphalt |
| `kick_tunnel.mp4` | `arteries_of_new_york_city.mp4` (Encyclopaedia Britannica / Prelinger Archives, 1941, Public Domain) | `00:03:24.0` – `00:03:25.2` | NYC subway train tunnel POV rushing into shadow (plus subtle archival subway thud layered into kick) |
| `rim_train.mp4` | `home_movie_098559_new_york_subway_platform_scenes.mp4` (Prelinger Home Movies, Public Domain) | `00:04:08.0` – `00:04:09.2` | Motion-blurred silver subway car windows streaking horizontally (plus archival subway foley room tail from `arteries_of_new_york_city.mp4`) |
| `hat_glitter.mp4` | `calvin_0302.mp4` (Prelinger Archives, Public Domain) | `00:02:58.5` – `00:03:00.5` | Out-of-focus night traffic lights & headlights bokeh orbs on pure black (plus subway turnstile clack in slice 4 from `arteries_of_new_york_city.mp4`) |
| `vocal_bench.mp4` | `home_movie_098559_new_york_subway_platform_scenes.mp4` (Prelinger Home Movies, Public Domain) | `00:11:54.0` – `00:12:12.0` | Solitary woman sitting on wooden bench at 7th Avenue subway station |
| `vocal_street.mp4` | `ask_me_don_t_tell_me_b_roll.mp4` (Prelinger Archives, 1961, Public Domain) | `00:01:50.1` – `00:01:55.5` (ping-pong loop) | Close-up profile on San Francisco twilight street corner |
| `holdout_forest.mp4` | `tranquil_forest.mp4` (CC0 / Public Domain stock) | `00:00:02.0` – `00:00:16.0` | Sunlight filtering through tall pine forest (held-out ambient benchmark) |
| `holdout_waterfall.mp4` | `forest_waterfall.mp4` (CC0 / Public Domain stock) | `00:00:01.0` – `00:00:13.0` | Misty waterfall cascading down mossy dark rock (held-out ambient benchmark) |
| `holdout_ocean.mp4` | `ocean_waves.mp4` (CC0 / Public Domain stock) | `00:00:00.5` – `00:00:08.5` | Turquoise ocean surf breaking on pale sand (held-out ambient benchmark) |

## 2. Original Generated Audio Stems (`gm` CLI — Gemini / Lyria API)

- **Female Soul/R&B Vocal Hook (`vocal_bench.mp4` audio)**:
  - Model: `lyria-3.5` (`--wav`)
  - Prompt: `"Solo female R&B soul vocal acapella in F minor, 67 bpm, intimate late-night vocal booth recording, NO drums, NO percussion, NO bass, almost completely solo dry-ish emotive voice with generous silence between phrases for sampling. Aching, breathy 2000s UK R&B singer singing slow melismatic hooks: 'Hold on... through the night... ooh, don't fade away... stay with me in the rain...'"`
- **Male Falsetto R&B Ad-libs (`vocal_street.mp4` slots 1–2)**:
  - Model: `lyria-3.5` (`--wav`)
  - Prompt: `"Solo male falsetto soul R&B vocal ad-libs in F minor, 67 bpm, isolated vocal stem, NO drums, NO percussion, NO bass, sparse pauses between phrases. Emotional breathy falsetto 'ooh-woah', 'in the dark', 'can't let go', 'waiting for you', raw aching soul vocals."`
- **Late-Night UK Spoken Voice Lines (`vocal_street.mp4` slots 3–4)**:
  - Model: `gemini-3.8-flash-tts`, voice `en-gb-storyteller-1`
    - Style: `"intimate, quiet, melancholic, late at night on an empty rainy London bus, slow with pauses"`
    - Prompt: `"I thought I saw your face through the rain... waiting on the platform... don't let the city take you."`
  - Model: `gemini-3.8-flash-tts`, voice `achernar`
    - Style: `"breathy, vulnerable, soft late-night whisper, slow and unhurried"`
    - Prompt: `"Stay here... until the streetlights go out... we're still awake."`
- **Nocturnal Dusty Rhodes & Pad Chords (`night_chords.mp4` audio, 0:00.2–0:12.2 drumless intro)**:
  - Model: `lyria-3-clip-preview` (`--wav`)
  - Prompt: `"30 second dark melancholic UK garage pad and dusty Rhodes electric piano chords in F minor, 67 bpm, NO drums, NO percussion, NO vocals. Warm analog tape saturation, deep aching Fm9 to DbMaj7 to AbMaj7 to Cm7 chords, nocturnal rainy South London mood."`
- **Nordic Chamber Ambient Texture (`holdout_forest.mp4`, `holdout_waterfall.mp4`, `holdout_ocean.mp4` audio)**:
  - Model: `lyria-3.5` (`--wav`)
  - Prompt: `"30 second acoustic cello harmonics, soft felted piano notes and wordless breathy choir hum in D dorian, 60 bpm, no drums, serene spacious nordic ambient chamber texture."`

## 3. Procedural / Foley-Layered Audio Stems
Synthesized in `outputs/261111_burial-demo/work/build_demo_assets.py` (deterministic Python DSP + archival subway foley from `arteries_of_new_york_city.mp4`):
- `rain_window.mp4`: Filtered stereo pink-noise rain swell + Poisson 12" vinyl dust hiss and warm needle pops.
- `sub_highway.mp4`: Pure mono F1 (`43.65 Hz`) sine sub fundamental + warm low-passed detuned F2 (`87.31 Hz`) stereo Reese movement.
- `kick_tunnel.mp4`: Pitch-enveloped UKG kick (`155 -> 47 Hz`) + archival subway tunnel low thud.
- `rim_train.mp4`: Multi-mode woody cross-stick/rimshot + flam'd dusty foley clap + archival station room tail.
- `hat_glitter.mp4`: 4-slice 2.0 s percussion strip (`0.00` closed hat/lighter, `0.25` open garage hat, `0.50` shaker/casing chink, `0.75` hollow woodblock + archival turnstile clack).
