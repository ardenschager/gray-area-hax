# Demo Media Attributions (`assets/demo/`)

All demo media in `assets/demo/` is public-domain archival footage paired with original generated or procedurally synthesized audio (zero copyrighted music or commercial samples). Total size: ~7.8 MB.

## 1. Archival Public-Domain / Open-License Video Sources
Trimmed and graded from the local `vibecut/videos/` archival collection:

| Demo File | Archival Video Source | Trim Window | Visual Content |
|---|---|---|---|
| `rain_window.mp4` | `rainy_window.mp4` (CC0 / Public Domain stock) | `00:00:08.0` – `00:00:20.0` | Raindrops sliding down dark windowpane against warm sodium/crimson streetlight bokeh |
| `night_chords.mp4` | `new_york_world_s_fair_part_3.mp4` (Prelinger Archives, Public Domain) | `00:53:44.0` – `00:53:56.0` | Nocturnal amber and cyan fountain reflections on black night sky |
| `sub_highway.mp4` | `calvin_0302.mp4` (Calvin Productions / Prelinger Archives, Public Domain) | `00:02:53.8` – `00:02:57.8` | Oncoming car headlights streaking across wet dark highway asphalt |
| `kick_tunnel.mp4` | `arteries_of_new_york_city.mp4` (Encyclopaedia Britannica / Prelinger Archives, 1941, Public Domain) | `00:03:24.0` – `00:03:25.2` | NYC subway train tunnel POV rushing into shadow |
| `rim_train.mp4` | `home_movie_098559_new_york_subway_platform_scenes.mp4` (Prelinger Home Movies, Public Domain) | `00:04:08.0` – `00:04:09.2` | Motion-blurred silver subway car windows streaking horizontally |
| `hat_glitter.mp4` | `calvin_0302.mp4` (Prelinger Archives, Public Domain) | `00:02:59.5` – `00:03:01.5` | Out-of-focus night traffic lights & headlights bokeh orbs on pure black |
| `subway_tunnel.mp4` | `arteries_of_new_york_city.mp4` (Prelinger Archives, 1941, Public Domain) | `00:03:22.2` – `00:03:28.2` | Forward-rushing dark 1941 NYC subway tunnel POV |
| `vocal_bench.mp4` | `home_movie_098559_new_york_subway_platform_scenes.mp4` (Prelinger Home Movies, Public Domain) | `00:11:54.0` – `00:12:06.0` | Solitary woman sitting on wooden bench at 7th Avenue subway station |
| `vocal_street.mp4` | `home_movie_098559_new_york_subway_platform_scenes.mp4` (Prelinger Home Movies, Public Domain) | `00:02:06.0` – `00:02:18.0` | Two standing overcoat silhouettes on 7th Avenue subway platform |
| `holdout_forest.mp4` | `tranquil_forest.mp4` (CC0 / Public Domain stock) | `00:00:02.0` – `00:00:16.0` | Sunlight filtering through tall pine forest (held-out ambient benchmark) |
| `holdout_waterfall.mp4` | `forest_waterfall.mp4` (CC0 / Public Domain stock) | `00:00:01.0` – `00:00:13.0` | Misty waterfall cascading down mossy dark rock (held-out ambient benchmark) |
| `holdout_ocean.mp4` | `ocean_waves.mp4` (CC0 / Public Domain stock) | `00:00:00.5` – `00:00:08.5` | Turquoise ocean surf breaking on pale sand (held-out ambient benchmark) |

## 2. Original Generated Audio Stems (`gm` CLI — Gemini / Lyria API)

- **Wordless Female Open-Vowel & Hum Stems (`vocal_bench.mp4` audio, 0:00–0:12)**:
  - Model: `lyria-3.5` (`--wav`) — strictly wordless open `"ooooh"` vowel melisma (`11.05–19.25s`) and soft closed-lip humming (`0.20–3.25s`) isolated with zero lyrics, preserving full natural breath & air up to `10.5 kHz`.
  - Generation prompt: `"Solo female R&B soul vocal acapella in F minor, 67 bpm, intimate late-night vocal booth recording, NO drums, NO percussion, NO bass, almost completely solo dry-ish emotive voice with generous silence between phrases for sampling..."`
- **Wordless Male Falsetto & Chest Vowel Stems (`vocal_street.mp4` audio, 0:00–0:12)**:
  - Model: `lyria-3.5` (`--wav`) — strictly wordless falsetto `"hooo / ooooh"` (`2.30–6.30s`), chest `"ohhh"` (`27.20–31.20s`), and breathy exhale (`13.35–15.95s`); zero sung lyrics or TTS speech.
  - Generation prompt: `"Solo male falsetto soul R&B vocal ad-libs in F minor, 67 bpm, isolated vocal stem, NO drums, NO percussion, NO bass, sparse pauses between phrases..."`
- **Nocturnal Dusty Rhodes & Pad Chords (`night_chords.mp4` audio, 0:00.2–0:12.2 drumless intro)**:
  - Model: `lyria-3-clip-preview` (`--wav`)
  - Prompt: `"30 second dark melancholic UK garage pad and dusty Rhodes electric piano chords in F minor, 67 bpm, NO drums, NO percussion, NO vocals. Warm analog tape saturation, deep aching Fm9 to DbMaj7 to AbMaj7 to Cm7 chords, nocturnal rainy South London mood."`
- **Nordic Chamber Ambient Texture (`holdout_forest.mp4`, `holdout_waterfall.mp4`, `holdout_ocean.mp4` audio)**:
  - Model: `lyria-3.5` (`--wav`)
  - Prompt: `"30 second acoustic cello harmonics, soft felted piano notes and wordless breathy choir hum in D dorian, 60 bpm, no drums, serene spacious nordic ambient chamber texture."`

## 3. Procedural Audio Stems
Synthesized in `outputs/261111_burial-demo/work/build_demo_assets.py` (deterministic 48 kHz Python DSP):
- `rain_window.mp4`: Filtered stereo pink-noise rain swell (`220 Hz – 9.2 kHz`), window droplet patters, and broadband 12" vinyl crackle (`1.85 – 9.6 kHz`).
- `sub_highway.mp4`: Pure mono F1 (`43.65 Hz`) sine sub fundamental + warm low-passed detuned F2 (`87.31 Hz`) stereo Reese movement.
- `kick_tunnel.mp4`: Pitch-enveloped UKG kick (`166 -> 48 Hz`) + dusty leather beater click.
- `rim_train.mp4`: Fast-settling woody cross-stick/rimshot + flam'd dusty foley clap/lighter snap (`520 Hz – 8.6 kHz`).
- `hat_glitter.mp4`: Crisp 4-slice 2.0 s UKG percussion strip (`0.00` closed hat/flint, `0.25` open garage hat, `0.50` matchbox shaker, `0.75` hollow turnstile knock; `2.6 – 12.6 kHz` top end).
- `subway_tunnel.mp4`: Rhythmic subterranean bogie wheel-on-rail-joint double-clack (`da-CLACK`) at 133 BPM + dark tunnel pneumatic air.
