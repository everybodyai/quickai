"""QuickAI's test of the environment ComfyUI was installed with: can Triton build what it builds, and run a kernel?

Triton, which PyTorch for a graphics card loads, compiles a small helper in C the first time a kernel is launched, with
the computer's C compiler and the headers of Python (`Python.h`); the models that run through kernels of their own
(the int8 and 4-bit builds of MiniMax H3, for one) need it on their first step. A computer that lacks the compiler or
the headers fails there, with a message that says only that the compiler "returned non-zero exit status 1" and keeps
the cause in the log of the server. This runs the same thing on a tiny kernel, from a cache that is empty (so that
the helper is built here and not found from an earlier run), before the engine is started.

It prints what it does, and ends with status 0 when a kernel was built and ran on the card, and with another when it
was not: 2 PyTorch cannot be imported, 3 it cannot use a card, 4 Triton cannot be imported, 1 a kernel could not be
built or run, 5 the kernel gave a wrong answer. What it says of a failure it says on the error stream, as the compiler
does, so that the compiler's own words and the sentence that follows them come in the order they were written: the
last lines are what the person is shown.
"""
import os
import shutil
import sys
import tempfile
import warnings

# a cache of its own, empty: Triton keeps what it builds in `~/.triton/cache`, and a helper built by an earlier run
# would stand in for the build this is here to try
CACHE = tempfile.mkdtemp(prefix="quickai-triton-")
os.environ["TRITON_CACHE_DIR"] = CACHE


def end(status):
    shutil.rmtree(CACHE, ignore_errors=True)
    sys.exit(status)


def fail(text, status):
    print(text, file=sys.stderr, flush=True)
    end(status)


try:
    import torch
except Exception as e:  # noqa: BLE001 - whatever stops the import is the answer
    fail(f"PyTorch could not be imported: {e}", 2)

with warnings.catch_warnings(record=True) as caught:
    warnings.simplefilter("always")
    usable = torch.cuda.is_available()
if not usable:
    for warning in caught:
        print(f"  {warning.message}", file=sys.stderr, flush=True)
    fail(
        f"PyTorch {torch.__version__} (built for {'CUDA ' + torch.version.cuda if torch.version.cuda else 'no card'}) "
        "cannot use a graphics card on this computer.",
        3,
    )

try:
    import triton
    import triton.language as tl
except Exception as e:  # noqa: BLE001
    fail(f"Triton could not be imported: {e}", 4)


@triton.jit
def add_one(x_ptr, out_ptr, n, BLOCK: tl.constexpr):
    offsets = tl.program_id(0) * BLOCK + tl.arange(0, BLOCK)
    mask = offsets < n
    tl.store(out_ptr + offsets, tl.load(x_ptr + offsets, mask=mask) + 1, mask=mask)


print(f"Building and running a kernel of Triton {triton.__version__} on {torch.cuda.get_device_name(0)}...", flush=True)
try:
    n = 1000
    x = torch.arange(n, device="cuda", dtype=torch.float32)
    out = torch.empty_like(x)
    add_one[(triton.cdiv(n, 256),)](x, out, n, BLOCK=256)
    torch.cuda.synchronize()
except Exception as e:  # noqa: BLE001
    said = f"{type(e).__name__}: {e}"
    fail(f"Triton could not build or run a kernel: {said[:300]}{'...' if len(said) > 300 else ''}", 1)

if not torch.equal(out, x + 1):
    fail("The kernel ran and gave a wrong answer.", 5)
print(f"ok: a kernel was built with the computer's compiler and ran on {torch.cuda.get_device_name(0)}", flush=True)
end(0)
