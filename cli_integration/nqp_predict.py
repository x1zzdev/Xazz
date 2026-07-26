#!/usr/bin/env python3
"""
nqp_predict.py — xazz CLI Integration: NQP Prediction Entry Point
==================================================================
Called by the xazz CLI when `--predict` flag is set.

Protocol:
  stdin  : JSON string  {"source": "<xazz source code>"}
           OR plain text (xazz source code, fallback)
  stdout : JSON string  {"status": "ok", "result": "...",
                         "confidence": "...", "warnings": [...]}
           OR error JSON {"status": "error", "message": "..."}

Usage (internal — called by Rust CLI):
  python nqp_predict.py
  (reads from stdin)
"""

import json
import os
import sys
import traceback

# ── Path setup ────────────────────────────────────────────────────────────────
_THIS_DIR    = os.path.dirname(os.path.abspath(__file__))
_PROJECT_DIR = os.path.dirname(_THIS_DIR)          # C:\Users\LG\xazz-lang
CHECKPOINT_DIR = r"C:\checkpoint-2814"             # NQP model checkpoint

# Make sure ir_converter is importable
sys.path.insert(0, _THIS_DIR)
from ir_converter import convert_xzz_to_ir, build_nqp_instruction  # noqa: E402


def emit_error(message: str) -> None:
    """Write error JSON to stdout and exit with code 1."""
    print(json.dumps({"status": "error", "message": message}, ensure_ascii=False))
    sys.exit(1)


def read_stdin_ir() -> dict:
    """
    Read IR / source from stdin.
    Accepts:
      - JSON: {"source": "..."}   — from Rust CLI
      - Plain text                — fallback
    Returns dict with at least {"source": "<code>"}
    """
    raw = sys.stdin.read()
    if not raw.strip():
        emit_error("No input received on stdin.")

    try:
        data = json.loads(raw)
        if isinstance(data, dict):
            return data
        # JSON but not a dict (e.g. plain JSON string)
        return {"source": str(data)}
    except json.JSONDecodeError:
        # Fallback: treat stdin as plain xazz source text
        return {"source": raw}


# ── Main ──────────────────────────────────────────────────────────────────────
def main():
    # 1. Read input
    data    = read_stdin_ir()
    source  = data.get("source", "")

    if not source.strip():
        emit_error("Empty source code received.")

    # 2. Convert to IR
    try:
        ir = convert_xzz_to_ir(source)
    except Exception as e:
        emit_error(f"IR conversion failed: {e}\n{traceback.format_exc()}")

    instruction, input_text = build_nqp_instruction(ir)

    # 3. Load NQP model ────────────────────────────────────────────────────────
    config_path = os.path.join(CHECKPOINT_DIR, "adapter_config.json")
    if not os.path.exists(config_path):
        emit_error(f"Checkpoint not found at: {CHECKPOINT_DIR}")

    with open(config_path, "r", encoding="utf-8") as f:
        adapter_cfg = json.load(f)

    BASE_MODEL = adapter_cfg.get("base_model_name_or_path")

    # Suppress transformer verbose logs to keep stdout clean for JSON protocol
    os.environ.setdefault("TRANSFORMERS_VERBOSITY", "error")
    os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")

    # Redirect stderr for noisy library warnings so stdout stays JSON-only
    import io
    _stderr_backup = sys.stderr
    sys.stderr = io.TextIOWrapper(
        open(os.devnull, "wb"), encoding="utf-8", errors="replace"
    )

    try:
        import torch  # noqa: F401
        DEVICE = "cuda" if torch.cuda.is_available() else "cpu"

        from transformers import AutoTokenizer, AutoModelForCausalLM  # noqa: E402

        # Load tokenizer from checkpoint dir (has tokenizer files)
        try:
            tokenizer = AutoTokenizer.from_pretrained(
                CHECKPOINT_DIR, trust_remote_code=True
            )
        except Exception:
            tokenizer = AutoTokenizer.from_pretrained(
                BASE_MODEL, trust_remote_code=True
            )

        if tokenizer.pad_token is None:
            tokenizer.pad_token = tokenizer.eos_token

        # Determine model loading kwargs
        load_kwargs = dict(trust_remote_code=True)

        BNB_AVAILABLE = False
        try:
            import bitsandbytes  # noqa: F401
            BNB_AVAILABLE = True
        except ImportError:
            pass

        if BNB_AVAILABLE and DEVICE == "cuda":
            from transformers import BitsAndBytesConfig
            bnb_config = BitsAndBytesConfig(
                load_in_4bit=True,
                bnb_4bit_compute_dtype=torch.float16,
                bnb_4bit_use_double_quant=True,
                bnb_4bit_quant_type="nf4",
            )
            load_kwargs["quantization_config"] = bnb_config
            load_kwargs["device_map"] = "auto"
            active_model_id = BASE_MODEL
        else:
            # CPU fallback — derive full-precision model id if needed
            active_model_id = BASE_MODEL
            if "-bnb-" in BASE_MODEL.lower():
                import re
                stripped = re.sub(r"-bnb-\d+bit$", "", BASE_MODEL, flags=re.IGNORECASE)
                if stripped.lower().startswith("unsloth/"):
                    stripped = stripped[len("unsloth/"):]
                    parts = stripped.split("-")
                    cased = "-".join(
                        p.title() if not p[0:1].isdigit() else p for p in parts
                    )
                    first = parts[0].lower()
                    if "qwen" in first:
                        active_model_id = f"Qwen/{cased}"
                    elif "llama" in first:
                        active_model_id = f"meta-llama/{cased}"
                    else:
                        active_model_id = cased
            load_kwargs["torch_dtype"] = torch.float32

        base_model = AutoModelForCausalLM.from_pretrained(active_model_id, **load_kwargs)

        # Attach LoRA adapter
        from peft import PeftModel  # noqa: E402
        model = PeftModel.from_pretrained(base_model, CHECKPOINT_DIR)
        model.eval()

    except Exception as e:
        sys.stderr = _stderr_backup
        emit_error(f"Model loading failed: {e}\n{traceback.format_exc()}")

    finally:
        sys.stderr = _stderr_backup

    # 4. Run inference ─────────────────────────────────────────────────────────
    SYSTEM_PROMPT = "You are the Neural Query Planner for Xazz."

    messages = [
        {"role": "system", "content": SYSTEM_PROMPT},
        {"role": "user",   "content": instruction + "\n\n" + input_text},
    ]

    try:
        prompt = tokenizer.apply_chat_template(
            messages, tokenize=False, add_generation_prompt=True
        )
    except Exception:
        prompt = (
            f"<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n"
            f"<|im_start|>user\n{instruction}\n\n{input_text}<|im_end|>\n"
            f"<|im_start|>assistant\n"
        )

    try:
        inputs    = tokenizer(prompt, return_tensors="pt").to(model.device)
        input_len = inputs["input_ids"].shape[1]

        with torch.no_grad():
            outputs = model.generate(
                **inputs,
                max_new_tokens=300,
                do_sample=False,
                temperature=None,
                top_p=None,
                pad_token_id=tokenizer.pad_token_id,
                eos_token_id=tokenizer.eos_token_id,
            )

        generated_ids = outputs[0][input_len:]
        raw_output = tokenizer.decode(generated_ids, skip_special_tokens=True).strip()
    except Exception as e:
        emit_error(f"Inference failed: {e}\n{traceback.format_exc()}")

    # 5. Build structured result ───────────────────────────────────────────────
    warnings = []

    # Simple heuristic confidence based on output length + keyword presence
    keywords = ["result", "output", "pipeline", "column", "data",
                "null", "filter", "group", "mean", "sum"]
    kw_hits  = sum(1 for k in keywords if k.lower() in raw_output.lower())
    if len(raw_output.strip()) < 20:
        confidence = "low"
        warnings.append("Output is very short — model may not have generated a useful response.")
    elif kw_hits >= 3:
        confidence = "high"
    elif kw_hits >= 1:
        confidence = "medium"
    else:
        confidence = "low"
        warnings.append("Expected domain keywords not found in model output.")

    if len(raw_output) > 1500:
        warnings.append("Output is unusually long — may contain repetition.")

    result_payload = {
        "status":     "ok",
        "result":     raw_output,
        "confidence": confidence,
        "warnings":   warnings,
        "ir_summary": {
            "ops_count":     len(ir["pipeline"]),
            "ops":           [o["op"] for o in ir["pipeline"]],
            "pipeline_text": ir["pipeline_text"],
        },
    }

    # Output must be pure JSON on stdout for the Rust layer to parse
    print(json.dumps(result_payload, ensure_ascii=False))


if __name__ == "__main__":
    main()
