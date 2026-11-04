# Models in DotCache: what uses them

2026-11-04. The maintainer asked which models in `/Volumes/DotCache` are in use by alexandria, disclosures, yapocis or any part of the PlumCraft stack.

There is no `/Volumes/DotCache`. `~/.cache` links to `/Volumes/UserCache/DotCache`, so that is what was searched.

**Scope.** The stack is the repos in `~/workspace/sysadmin/config/stack.toml`, plus plumcraft-duckdb and atlas-engine:

- yapocis3, yapocis-rs, yapocis_external;
- impressionist, impressionist-rs, impcraft;
- alexandria, including disclosures, and its `alexandria-zenodotus` worktree;
- virtimp.com, pipekit, pycommonkit, upath-rclone;
- plumcraft, plumcraft-virtimp, opencv-engine, sysadmin.

The craft apps' source (`~/workspace-ex/artcraft`) is on a volume that isn't mounted. The craft binaries on PATH were searched instead.

**Method.** Each model's repo and file names were searched across every repo, then followed to the code that loads them:

- `MODEL_STORE`, `HF_HOME`, `TORCH_HOME`;
- alexandria's `.env` and its manifests;
- pipekit's model loaders;
- the yapocis model-store lookup order.

`~/workspace/sysadmin/docs/models-readme.md` already describes yellowfin's `/data/models`. On this Mac, alexandria's `.env` sets `MODEL_STORE=$HOME/.cache/alexandria/models` and `MODEL_SOURCE=rclone://yellowfin/data/models`. So `~/.cache/alexandria/models` is the Mac's local copy of that store, filled on first use.

## Summary

| Status | Size | Models |
|---|---|---|
| In use | about 12 GB (plus 626 MB in yapocis3, by symlink) | DeepSeek-OCR-2 8-bit (MLX), CLIP ViT-L/14, Florence-2-large, the CLIP face-attribute head, the three CLIP probes and adapter, the restore bundle (LaMa, the scratch detector, GFPGAN, DDColor), torchvision VGG16, the yapocis ResNet-50, VGG16 and YuNet |
| Build inputs, not read at run time | about 12.6 GB | DeepSeek-OCR-2 bf16 release and the dequantized build |
| Duplicates of in-use models | about 1.4 GB | a second VGG16 checkpoint, and DDColor in the HF cache |
| Not used by anything in the stack | about 11.4 GB | ComfyUI's models, the MLX Whisper models, Surya OCR, the dormant torchvision detectors, Whisper base.en, and two empty HF stubs |

## In use

| Model | Where | Size | Used by |
|---|---|---|---|
| DeepSeek-OCR-2, 8-bit MLX | `alexandria/models/mlx-community/DeepSeek-OCR-2-8bit` | 3.8 G | **disclosures**: the Mac handwriting worker, `HandwritingInfer` in `pipelines/manifests/disclosures-mac.yml`, `model_root: ${MODEL_STORE}` |
| CLIP ViT-L/14 | `huggingface/hub/models--openai--clip-vit-large-patch14` | 1.6 G | **alexandria**, through pipekit `models/clip.py` (ClipInfer) and `models/face_attr.py`, the face-attribute base. It is also the feature space of the three probes below |
| Florence-2-large | `huggingface/hub/models--microsoft--Florence-2-large` | 1.4 G | **alexandria**: OCR and captions, through pipekit `models/florence.py` (FlorenceInfer in `alexandria-dev.yml`) |
| CLIP face-attribute classifier | `huggingface/hub/models--syntheticbot--clip-face-attribute-classifier` | 1.1 G | **alexandria**: face attributes, through pipekit `models/face_attr.py` |
| Page-type probe, photo probe, CLIP adapter | `alexandria/models/{page-type-probe,photo-probe,clip_adapters}` | 12 M | **alexandria**: `PAGE_TYPE_MODEL`, `PHOTO_PROBE_MODEL` and `CLIP_ADAPTER_DIR` in `pipelines/env.py`. They exist nowhere else except yellowfin, so keep them |
| LaMa (`big-lama.pt`) | `alexandria/models/torch-checkpoints/` | 196 M | **alexandria** restore `repair` (`RESTORE_MODEL`) |
| Scratch detector (`bopbl/Global`) | `alexandria/models/bopbl` | 2.1 G | **alexandria** restore `repair` (`RESTORE_CKPT`; its code is `RESTORE_DETECTOR_HOME`, set to `alexandria/claude-debug/bopbl` in `.env.runtime`) |
| GFPGAN v1.4, with RetinaFace and ParseNet | `alexandria/models/gfpgan` | 518 M | **alexandria** restore `gfpgan` (`RESTORE_GFPGAN_MODEL`; the flat layout is required) |
| DDColor | `alexandria/models/ddcolor/ddcolor_modelscope.bin` | 870 M | **alexandria** restore `ddcolor` (`RESTORE_DDCOLOR_MODEL`) |
| torchvision VGG16 | `torch/hub/checkpoints/vgg16-397923af.pth` | 528 M | **alexandria** EmbedInfer, through pipekit `models/vgg16.py`, `vgg16(weights=IMAGENET1K_V1)`. `TORCH_HOME` isn't set, so torchvision reads this default cache |
| yapocis ResNet-50 and VGG16 (`weights.npz`) | `yapocis/models/{resnet50,vgg16}` | links | **yapocis, impcraft, PlumImpress:** Classify, and the VGG16/ResNet-50 tests. These are symlinks to `yapocis3/src/yapocis/models/*/weights.npz` (98 M and 528 M), installed by `impcraft-cli models install` |
| YuNet | `yapocis/models/yunet` | 248 K | **yapocis_external** `lib/yunet.py`, impressionist-rs face detection, PlumImpress's model list |

## Build inputs, not read at run time

| Model | Where | Size | Role |
|---|---|---|---|
| DeepSeek-OCR-2, bf16 release | `alexandria/models/deepseek-ai/DeepSeek-OCR-2` | 6.3 G | Source for the dequantized build (`pipelines/disclosures/handwriting_weights.py`, run in the Mac's MLX venv) |
| DeepSeek-OCR-2, dequantized to bf16 | `alexandria/models/alexandria/DeepSeek-OCR-2-mlx8-dequant` | 6.3 G | **disclosures'** CUDA handwriting reader reads its copy on yellowfin (`disclosures-cuda.yml`). Nothing on the Mac reads this one; it is where the build writes |

Both are on yellowfin too. Here they matter only to rebuild the dequantized copy.

## Duplicates of models in use

| Model | Where | Size | Why it's a duplicate |
|---|---|---|---|
| VGG16 | `alexandria/models/torch-checkpoints/vgg16-397923af.pth` | 528 M | The same file as `torch/hub/checkpoints/`. With no `TORCH_HOME` set, nothing on the Mac reads this copy |
| DDColor | `huggingface/hub/models--piddnad--ddcolor_modelscope` | 870 M | The restore op reads `alexandria/models/ddcolor/` (`RESTORE_DDCOLOR_MODEL`). This is where the weights were downloaded from |

## Not used by anything in the stack

| Model | Where | Size | What it was |
|---|---|---|---|
| ComfyUI's Real-ESRGAN, SDPose, BiRefNet, MoGe 2 and Depth Anything 3, and Kijai's SAM 2.1 large and small | `huggingface/hub/models--Comfy-Org--*`, `models--Kijai--sam2-safetensors` | 4.8 G together (shared `hub/blobs`) | Fetched 2026-10-04 and 10-16 by `impressionist/claude-debug/comfy_fetch_models.py` for `~/workspace/ComfyUI`, for comfy-impressionist. ComfyUI is no longer installed, comfy-impressionist is retired (impcraft `AGENTS.md`), and no stack code names these. The craft apps download their own SAM 2.1 from `facebook/sam2.1-hiera-*`, not these |
| MLX Whisper large-v3, large-v3-turbo and tiny | `huggingface/hub/models--mlx-community--whisper-*` | 4.5 G | Fetched 2025-12-27. No stack repo names them. Disclosures' ASR is faster-whisper on yellowfin (pipekit `models/asr.py`). `whisper-srt/subtitle.py`, not in the stack, uses openai-whisper large-v3-turbo |
| Surya OCR 2 (GGUF) | `huggingface/hub/models--datalab-to--surya-ocr-2-gguf`, and `datalab/` | 1.4 G | Fetched 2026-06-20. Its only mention is `alexandria/claude-debug/PIPELINE-COMPARISON-20260417.md`, a one-off comparison |
| torchvision ResNet-50, RetinaNet R50-FPN v1 and v2, Faster R-CNN R50-FPN | `alexandria/models/torch-checkpoints/` | 534 M | Dormant on yellowfin too (`models-readme.md` section 3: "Only `vgg16` and `big-lama` are live"). They came over with the store mirror |
| Whisper base.en | `whisper/base.en.pt` | 139 M | Fetched 2026-11-01 by a PlumCraft Claude session, a one-off `uv tool run --from openai-whisper whisper … --model base.en` transcription. No code uses it |
| Empty HF stubs | `models--coreml-projects--coreml-sam2-large`, `models--mlx-community--DeepSeek-OCR-2-8bit` | about 0 | The SAM 2 Core ML model was downloaded with `--local-dir` into `yapocis_external/models/`. DeepSeek-OCR-2 is read as a plain directory from `alexandria/models`. Only the refs remain |

## Not models

These were not counted:

- `pipekit/venvs` (575 M): the pipekit worker venvs, such as `~/.cache/pipekit/venvs/mlx` for the handwriting worker.
- `yapocis/<hash>` and `yapocis/ocl` (about 9 M): compiled-kernel caches.
- `uv` (2.3 G): the package cache.

## Caveats

- **The craft apps' source wasn't searched.** It is on an unmounted volume. Their binaries name only AuraFace (LightCraft), Kokoro (FilmCraft) and SAM 2.1 (EffectCraft), and those go to the crafts' own folders, not to these.
- **"In use" means code and config read the file today.** That doesn't mean a worker has run lately. Alexandria's Mac dev fleet (`alexandria-dev.yml`) runs the Florence, Embed, CLIP, Face and Restore workers only when it is started.
- **Sizes:** each model's own blobs; Comfy-Org and Kijai share `hub/blobs`.
- **Nothing was deleted or moved.**

## Removed (2026-11-04)

The maintainer: "Remove the ComfyUI downloaded models, the unused Whisper models, the Surya model, the unused torchvision models, and Whisper base.en.pt". Removed:

- **The six ComfyUI repos:** `models--Comfy-Org--{BiRefNet,Depth-Anything-3,MoGe,Real-ESRGAN_repackaged,SDPose}` and `models--Kijai--sam2-safetensors`, with their 10 files in the shared `hub/blobs`. No other repo's files resolved into those blobs.
- **The MLX Whisper repos:** `models--mlx-community--whisper-{large-v3-mlx,large-v3-turbo,tiny}`.
- **Surya:** `models--datalab-to--surya-ocr-2-gguf`. `datalab/surya/llamacpp_server.log`, 16 K, is left.
- **The unused torchvision models:** `alexandria/models/torch-checkpoints/{resnet50-0676ba61,retinanet_resnet50_fpn_coco-eeacb38b,retinanet_resnet50_fpn_v2_coco-5905b1c5,fasterrcnn_resnet50_fpn_coco-258fb6c6}.pth`. `big-lama.pt` and `vgg16-397923af.pth` stay.
- **`whisper/base.en.pt`.**

Free space on the volume went from 77.7 to 88.9 GiB, so about 11.2 GiB was freed. That is approximate, because worktree builds were writing to the same disk.

**Left, as not asked:**

- the duplicate VGG16 and DDColor copies;
- the two empty HF stubs;
- 27 small orphan files in `hub/blobs` that nothing links to.

The torchvision detectors are still on yellowfin's `/data/models`. Alexandria's `MODEL_SOURCE` sync fetches only what a worker needs, and no worker uses these.

**Also removed (2026-11-04),** after "Remove the duplicate VGG16 and DDColor copies too":

- `alexandria/models/torch-checkpoints/vgg16-397923af.pth`. The live copy is `torch/hub/checkpoints/vgg16-397923af.pth`, which pipekit's `vgg16()` reads through torchvision's default cache.
- `huggingface/hub/models--piddnad--ddcolor_modelscope`. The live copy is `alexandria/models/ddcolor/ddcolor_modelscope.bin`. pipekit's colorizer loads it by path (`RESTORE_DDCOLOR_MODEL`), never from the HF cache.

Free space went from 81.8 to 83.1 GiB.

**The `torch-checkpoints` marker was rewritten, with the maintainer's agreement.**

- **Why:** alexandria's `ensure_local` treats a folder as complete when its files match the count and size in `.alexandria-materialized.json`. That file still said 6 files and 1,318,996,785 bytes. The next restore run would therefore have copied all six checkpoints back from yellowfin.
- **The change:** the marker now records the one file left, `big-lama.pt` (205,803,670 bytes), and adds a `note` that says what was pruned.
- **Checked:** `alexandria.storage.materialize.is_materialized` returns True for the folder.
- **The old marker:** kept outside the folder, because any file inside it is counted. It is in the session scratchpad as `torch-checkpoints.alexandria-materialized.json.bak`.
