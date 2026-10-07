cd /data/local/tmp/ocr
export LD_LIBRARY_PATH=. REC_BATCH=1 MEM_PATTERN=0 RAYON_NUM_THREADS=4
IMGS="img/privacy-policy.zh-1.png img/privacy-policy.zh-2.png img/privacy-policy-3.png img/zh-scan-2.jpg"
OCR="m/pp-ocrv6_tiny_det.onnx m/pp-ocrv6_tiny_rec.onnx m/ppocrv6_tiny_dict.txt"
pre() { sleep 40; echo "== $1  [free: $(dumpsys meminfo | grep 'Free RAM' | sed 's/ *Free RAM: *//; s/ (.*//')]"; }
f() { grep -E "^(load|warm|page|avg|pdf|Error)|Killed|rendered" | sed 's/ kinds=.*rss/ rss/'; }
pre "pdfium, all cores";            ./ocr-bench pdf . 200 out/pdf img/privacy-policy.zh.pdf img/combo45.pdf 2>&1 | f
pre "pdfium, silver cores";         taskset 0f ./ocr-bench pdf . 200 out/pdf img/combo45.pdf 2>&1 | f
pre "OCR v6 tiny, gold x4";         taskset f0 ./ocr-bench ocr $OCR 4 out/ocr-gold $IMGS 2>&1 | f
pre "OCR v6 tiny, silver x4";       taskset 0f ./ocr-bench ocr $OCR 4 out/ocr-silver $IMGS 2>&1 | f
pre "OCR v6 tiny, all 8, 8 threads"; RAYON_NUM_THREADS=8 taskset ff ./ocr-bench ocr $OCR 8 out/ocr-all $IMGS 2>&1 | f
pre "layout S only, gold x4";       NO_OCR=1 NO_TABLE=1 taskset f0 ./ocr-bench structure m/pp-doclayout-s.onnx pp_doclayout_s $OCR m/slanet_plus.onnx m/table_structure_dict_ch.txt 4 out/ls $IMGS 2>&1 | f
pre "structure S full, gold x4";    taskset f0 ./ocr-bench structure m/pp-doclayout-s.onnx pp_doclayout_s $OCR m/slanet_plus.onnx m/table_structure_dict_ch.txt 4 out/ss $IMGS 2>&1 | f
pre "structure M full (tables), gold x4"; taskset f0 ./ocr-bench structure m/pp-doclayout-m.onnx pp_doclayout_m $OCR m/slanet_plus.onnx m/table_structure_dict_ch.txt 4 out/sm $IMGS 2>&1 | f
echo "== ALL DONE"
