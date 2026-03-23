#!/usr/bin/env python3
"""Fix SenseVoiceSmall ONNX export type mismatches.

FunASR's torch.onnx.export produces Less nodes with mixed f32/i64 inputs.
This script inserts Cast nodes to fix the type mismatch so OpenVINO can load it.
"""
import sys
import time
import onnx
from onnx import helper, TensorProto, numpy_helper

MODEL_DIR = "/home/yggdrasil/.cache/modelscope/hub/models/iic/SenseVoiceSmall"
MODEL_PATH = f"{MODEL_DIR}/model.onnx"
FIXED_PATH = f"{MODEL_DIR}/model_fixed.onnx"


def get_elem_type(name, type_map):
    return type_map.get(name)


def build_type_map(graph):
    """Map tensor names to their element types from graph inputs, initializers, and node outputs."""
    tmap = {}
    for inp in graph.input:
        t = inp.type.tensor_type.elem_type
        if t:
            tmap[inp.name] = t
    for init in graph.initializer:
        tmap[init.name] = init.data_type
    # Infer output types from node op_type where possible
    for node in graph.node:
        if node.op_type == "Cast":
            for attr in node.attribute:
                if attr.name == "to":
                    for out in node.output:
                        tmap[out] = attr.i
        elif node.op_type == "Shape":
            for out in node.output:
                tmap[out] = TensorProto.INT64
        elif node.op_type == "Range":
            # Range output type matches first input
            if node.input[0] in tmap:
                for out in node.output:
                    tmap[out] = tmap[node.input[0]]
        elif node.op_type == "Convert":
            # FunASR custom op - check attributes
            for out in node.output:
                tmap[out] = TensorProto.INT64  # conservative
        elif node.op_type in ("Unsqueeze", "Reshape", "Gather", "Slice"):
            if node.input[0] in tmap:
                for out in node.output:
                    tmap[out] = tmap[node.input[0]]
    return tmap


def fix_comparison_nodes(graph):
    """Insert Cast nodes before comparison ops (Less, Greater, Equal) with mixed types."""
    type_map = build_type_map(graph)
    fixes = 0
    new_nodes = []

    for node in graph.node:
        if node.op_type in ("Less", "Greater", "Equal", "LessOrEqual", "GreaterOrEqual"):
            if len(node.input) == 2:
                t0 = get_elem_type(node.input[0], type_map)
                t1 = get_elem_type(node.input[1], type_map)

                if t0 is not None and t1 is not None and t0 != t1:
                    # Cast the non-float input to float
                    target = TensorProto.FLOAT
                    for j in range(2):
                        inp_t = get_elem_type(node.input[j], type_map)
                        if inp_t != target:
                            cast_out = f"{node.input[j]}_cast_f32_{fixes}"
                            cast_node = helper.make_node(
                                "Cast", [node.input[j]], [cast_out], to=target
                            )
                            new_nodes.append(cast_node)
                            node.input[j] = cast_out
                            type_map[cast_out] = target
                            fixes += 1

        new_nodes.append(node)

    if fixes > 0:
        del graph.node[:]
        graph.node.extend(new_nodes)

    return fixes


def main():
    print(f"Loading ONNX model from {MODEL_PATH}...")
    t0 = time.time()
    model = onnx.load(MODEL_PATH, load_external_data=True)
    print(f"Loaded in {time.time()-t0:.1f}s, {len(model.graph.node)} nodes")

    fixes = fix_comparison_nodes(model.graph)
    print(f"Applied {fixes} type-cast fixes")

    if fixes == 0:
        print("No fixes needed. Exiting.")
        sys.exit(0)

    print(f"Saving fixed model to {FIXED_PATH}...")
    onnx.save(model, FIXED_PATH)
    size_mb = sum(1 for _ in open(FIXED_PATH, "rb")) if False else 0
    print(f"Saved. Verifying...")

    # Quick verify: check the model loads
    try:
        import onnxruntime as ort
        sess = ort.InferenceSession(FIXED_PATH, providers=["CPUExecutionProvider"])
        print(f"Verification OK - inputs: {[(i.name, i.shape) for i in sess.get_inputs()]}")
        del sess
    except Exception as e:
        print(f"Verification failed: {e}")
        sys.exit(1)

    print("Done.")


if __name__ == "__main__":
    main()
