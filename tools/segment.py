#!/usr/bin/env python3
"""SAM 3 sidecar for chromagrain: text-prompted video object segmentation.

chromagrain shells out to this script the same way it shells out to
yt-dlp/ffmpeg — the model never lives inside the Rust process.

Usage:
    python3 tools/segment.py <video.mp4> "<prompt>" <out_dir>
    python3 tools/segment.py --check

Contract (what the Rust side relies on):
  * one grayscale binary PGM (P5, maxval 255) per input frame, named
    mask_00000.pgm, mask_00001.pgm, ... in <out_dir>; 255 = object.
    Masks are the union of every instance matching the prompt (every
    "cat" in frame). Mask dimensions should match the video frames but
    the reader resamples if they don't.
  * exit 0 with "ok <n_frames>" on the last stdout line on success;
    nonzero with a human-readable message on stderr on failure.
  * --check exits 0 iff SAM 3 is importable (prints its device).

Setup (once):
    pip install torch torchvision  # CUDA build; see pytorch.org
    git clone https://github.com/facebookresearch/sam3 && pip install -e sam3
    # request checkpoint access at https://huggingface.co/facebook/sam3
    hf auth login

An alternative segmenter can replace this script entirely by setting
CHROMAGRAIN_SEGMENT_CMD to any command with the same CLI + contract.
"""

import sys


def write_pgm(path, arr):
    """Write a 2D uint8 numpy array as a binary PGM (P5). No PIL needed."""
    h, w = arr.shape
    with open(path, "wb") as f:
        f.write(f"P5\n{w} {h}\n255\n".encode("ascii"))
        f.write(arr.tobytes())


def check():
    try:
        import torch  # noqa: F401
        from sam3.model_builder import build_sam3_video_predictor  # noqa: F401
    except ImportError as e:
        print(f"sam3 unavailable: {e}", file=sys.stderr)
        return 3
    import torch

    device = "cuda" if torch.cuda.is_available() else "cpu"
    print(f"sam3 ok ({device})")
    return 0


def segment(video_path, prompt, out_dir):
    import os

    import numpy as np
    from sam3.model_builder import build_sam3_video_predictor

    os.makedirs(out_dir, exist_ok=True)

    predictor = build_sam3_video_predictor()
    response = predictor.handle_request(
        request=dict(type="start_session", resource_path=video_path)
    )
    session_id = response["session_id"]

    predictor.handle_request(
        request=dict(
            type="add_prompt",
            session_id=session_id,
            frame_index=0,
            text=prompt,
        )
    )

    n = 0
    for response in predictor.handle_stream_request(
        request=dict(type="propagate_in_video", session_id=session_id)
    ):
        frame_index = response["frame_index"]
        out = response["outputs"]
        masks = out["out_binary_masks"]
        union = None
        for m in masks:
            m = np.asarray(m).astype(bool)
            union = m if union is None else (union | m)
        if union is None:
            # Nothing matched on this frame: fully transparent.
            union = np.zeros((2, 2), dtype=bool)
        arr = (union.astype(np.uint8)) * 255
        if arr.ndim == 3:  # squeeze a leading 1-channel if present
            arr = arr.reshape(arr.shape[-2], arr.shape[-1])
        write_pgm(os.path.join(out_dir, f"mask_{frame_index:05d}.pgm"), arr)
        n = max(n, frame_index + 1)

    print(f"ok {n}")
    return 0


def main(argv):
    if len(argv) == 2 and argv[1] == "--check":
        return check()
    if len(argv) != 4:
        print(__doc__, file=sys.stderr)
        return 2
    return segment(argv[1], argv[2], argv[3])


if __name__ == "__main__":
    sys.exit(main(sys.argv))
