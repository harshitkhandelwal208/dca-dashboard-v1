#!/usr/bin/env sh
# Downloads the PaddleOCR (PP-OCRv5) ONNX models the bot reads screenshots with.
# Usage: bot/scripts/download-models.sh [target-dir]   (default: bot/models)
set -eu
DIR="${1:-$(dirname "$0")/../models}"
BASE="${DCA_MODEL_BASE_URL:-https://github.com/GreatV/oar-ocr/releases/download/v0.3.0}"
mkdir -p "$DIR"
for f in \
  pp-ocrv5_mobile_det.onnx \
  pp-ocrv5_mobile_rec.onnx ppocrv5_dict.txt \
  latin_pp-ocrv5_mobile_rec.onnx ppocrv5_latin_dict.txt \
  cyrillic_pp-ocrv5_mobile_rec.onnx ppocrv5_cyrillic_dict.txt \
  arabic_pp-ocrv5_mobile_rec.onnx ppocrv5_arabic_dict.txt \
  korean_pp-ocrv5_mobile_rec.onnx ppocrv5_korean_dict.txt \
  th_pp-ocrv5_mobile_rec.onnx ppocrv5_th_dict.txt \
  el_pp-ocrv5_mobile_rec.onnx ppocrv5_el_dict.txt \
  devanagari_pp-ocrv5_mobile_rec.onnx ppocrv5_devanagari_dict.txt \
  ta_pp-ocrv5_mobile_rec.onnx ppocrv5_ta_dict.txt \
  te_pp-ocrv5_mobile_rec.onnx ppocrv5_te_dict.txt \
  pp-lcnet_x1_0_doc_ori.onnx ; do
  if [ ! -s "$DIR/$f" ]; then
    echo "downloading $f"
    curl -fsSL --retry 3 -o "$DIR/$f.part" "$BASE/$f"
    mv "$DIR/$f.part" "$DIR/$f"
  fi
done
echo "models ready in $DIR"
