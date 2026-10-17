# /// script
# requires-python = ">=3.11"
# dependencies = ["pydantic-ai-slim[openrouter]>=2.51,<3"]
# ///
"""Translate the interface strings through OpenRouter.

Every src/lib/assets/locale/{lang}/{file}.json is rebuilt from
locale/en/{file}.json, so its keys cannot drift from the English ones. Only the
English values are sent to the model, as numbered items. Keys never leave this
script. Chinese and Japanese go to GLM, every other language to Gemini.

    export OPENROUTER_API_KEY=...
    uv run tools/locale/translate.py                    # every language, every file
    uv run tools/locale/translate.py --lang fr,de
    uv run tools/locale/translate.py --file common,editor
    uv run tools/locale/translate.py --missing          # only strings not translated yet
    uv run tools/locale/translate.py --dry-run          # count what would be sent
    uv run tools/locale/translate.py --check            # check the files on disk, no API
    uv run tools/locale/translate.py --folder vendor/zsuite/packages/assistant/src/locale

scripts/translate_locale does the same with the key pulled from Bitwarden.
"""

import argparse
import asyncio
import json
import os
import pathlib
import re
import sys
from dataclasses import dataclass

ROOT = pathlib.Path(__file__).resolve().parents[2]
LOCALE = ROOT / "src" / "lib" / "assets" / "locale"
SOURCE = "en"

GLM = "z-ai/glm-5.3-flash"
GEMINI = "google/gemini-3.1-flash-lite"
# Thinking is billed as output. The lowest effort each model accepts, from the
# "reasoning" field of openrouter.ai/api/v1/models: GLM cannot turn it off and
# defaults to "max", Gemini defaults to "minimal".
REASONING = {GLM: "low", GEMINI: "minimal"}
LANGS = {
    "fr": ("French", GEMINI),
    "de": ("German", GEMINI),
    "nl": ("Dutch", GEMINI),
    "es": ("Spanish", GEMINI),
    "pt": ("Portuguese (Portugal)", GEMINI),
    "it": ("Italian", GEMINI),
    "nb": ("Norwegian Bokmål", GEMINI),
    "sv": ("Swedish", GEMINI),
    "da": ("Danish", GEMINI),
    "el": ("Greek", GEMINI),
    "ru": ("Russian", GEMINI),
    "uk": ("Ukrainian", GEMINI),
    "pl": ("Polish", GEMINI),
    "tr": ("Turkish", GEMINI),
    "he": ("Hebrew", GEMINI),
    "ar": ("Arabic", GEMINI),
    "fa": ("Persian (Farsi)", GEMINI),
    "zh-CN": ("Simplified Chinese (mainland China)", GLM),
    "zh-TW": ("Traditional Chinese (Taiwan)", GLM),
    "ja": ("Japanese", GLM),
}
PLACEHOLDER = re.compile(r"\{[^{}]+\}")
CONCURRENCY = 6
ATTEMPTS = 3


@dataclass
class Job:
    lang: str
    file: str
    skeleton: dict
    leaves: dict  # {path: English text}, every string of the file
    todo: list  # the paths this run translates

    @property
    def out(self):
        return LOCALE / self.lang / f"{self.file}.json"


def flatten(value, prefix=()):
    """{path: text} for every string in a nested dict."""
    if isinstance(value, str):
        return {prefix: value}
    return {path: text for key, child in value.items() for path, text in flatten(child, prefix + (key,)).items()}


def build(skeleton, values, path=()):
    """The skeleton with each string replaced by values[path]; untranslated strings are left out."""
    if isinstance(skeleton, str):
        return values.get(path)
    out = {key: build(child, values, path + (key,)) for key, child in skeleton.items()}
    kept = {key: value for key, value in out.items() if value is not None}
    return kept if kept or not path else None


def faithful(english, text):
    """Whether a translation keeps the placeholders and the line breaks of the English."""
    return (
        sorted(PLACEHOLDER.findall(english)) == sorted(PLACEHOLDER.findall(text))
        and english.count("\n") == text.count("\n")
        and "\\n" not in text
    )


def existing(out):
    return flatten(json.loads(out.read_text())) if out.exists() else {}


def plan(args):
    langs = args.lang.split(",") if args.lang else list(LANGS)
    if unknown := [lang for lang in langs if lang not in LANGS]:
        sys.exit(f"no such language: {', '.join(unknown)} (one of {', '.join(LANGS)})")
    known = sorted(p.stem for p in (LOCALE / SOURCE).glob("*.json"))
    files = args.file.split(",") if args.file else known
    if unknown := [file for file in files if file not in known]:
        sys.exit(f"no such file: {', '.join(unknown)} (one of {', '.join(known)})")

    jobs = []
    for lang in langs:
        for file in files:
            skeleton = json.loads((LOCALE / SOURCE / f"{file}.json").read_text())
            job = Job(lang, file, skeleton, flatten(skeleton), [])
            done = existing(job.out) if args.missing else {}
            # a string that lost a placeholder or a line break counts as missing
            job.todo = [path for path, text in job.leaves.items() if path not in done or not faithful(text, done[path])]
            # a file with keys English no longer has is rewritten too, which drops them
            if job.todo or done.keys() - job.leaves.keys() or args.check:
                jobs.append(job)
    return jobs


def problems(job):
    """What is wrong with a written locale file, measured against its English source."""
    got = existing(job.out)
    found = []
    if extra := sorted(got.keys() - job.leaves.keys()):
        found.append(f"{len(extra)} key(s) not in English, e.g. {'.'.join(extra[0])}")
    if missing := sorted(job.leaves.keys() - got.keys()):
        found.append(f"{len(missing)} string(s) untranslated, e.g. {'.'.join(missing[0])}")
    for path in job.leaves.keys() & got.keys():
        where = ".".join(path)
        if not got[path].strip():
            found.append(f"{where} is empty")
        elif not faithful(job.leaves[path], got[path]):
            found.append(f"{where} does not keep the placeholders or line breaks of {job.leaves[path]!r}")
    return found


def report(jobs):
    bad = 0
    for job in jobs:
        found = problems(job)
        bad += bool(found)
        for line in found:
            print(f"  {job.out.relative_to(ROOT)}: {line}")
    return bad


def agent():
    os.environ.setdefault("PYDANTIC_AI_NO_BANNER", "1")
    from pydantic import BaseModel
    from pydantic_ai import Agent, ModelRetry, RunContext

    class Item(BaseModel):
        id: int
        text: str

    class Translation(BaseModel):
        items: list[Item]

    @dataclass
    class Batch:
        language: str
        sent: dict  # {id: English text}

    settings = {
        "temperature": 0.2,
        "timeout": 180,
        # GLM is served by dozens of providers at up to 7x the listed price
        "openrouter_provider": {"sort": "price"},
    }
    translator = Agent(output_type=Translation, deps_type=Batch, retries=3, model_settings=settings)

    @translator.instructions
    def instructions(ctx: RunContext[Batch]) -> str:
        language = ctx.deps.language
        return f"""You translate the English interface text of znote into {language}. znote is a Windows desktop app for people who are not programmers: they write documents in Typst on the left and see the pages on the right. Documents are pages, kept in notebooks, kept in projects. An AI assistant can write and edit pages for them.
The input is a JSON list of items, each with an id, a context saying where the text is used, and the English text.
Return every id exactly once, with its {language} translation as the text.
- Use the words {language} editions of Windows and office software use (Save, Cancel, Settings, Recycle Bin, folder). Match the length and register of the English: short labels stay short, sentences stay sentences. Address the user the way software in {language} usually does.
- Copy every placeholder in curly braces, such as {{count}}, exactly as it is. Never translate or drop one. A placeholder stands for a name, a number or a message put in at run time.
- Keep these unchanged: znote, Typst, Claude, Claude Code, OpenRouter, Ollama, LM Studio, OpenAI, PowerPoint, PDF, API, Pro, Max, Team, and key names such as Ctrl+S and Ctrl+Enter.
- Keep the trailing … of menu items and progress messages, and use the quotation marks {language} uses where the English has “ ”.
- Keep every line break and bullet mark (•) where it is. In Typst source, such as "= Welcome to znote" or "*Bold*, _emphasis_ and `code`", translate only the words and keep the markup and the maths between $ signs exactly.
- A context starting with "backend." is an error message. Text that starts with a small letter is the end of another sentence; keep it that way.
- A context ending in _one or _other is the singular or plural form of the same text.
- The context is only there to help; never translate it or add notes to the text."""

    @translator.output_validator
    def validate(ctx: RunContext[Batch], output: Translation) -> Translation:
        got = {item.id: item.text for item in output.items}
        if sorted(got) != sorted(ctx.deps.sent) or len(output.items) != len(got):
            raise ModelRetry(f"return exactly one item for each of the ids {sorted(ctx.deps.sent)}")
        if empty := [i for i, text in got.items() if not text.strip()]:
            raise ModelRetry(f"items {empty} are empty; translate them")
        if wrong := [i for i, text in ctx.deps.sent.items() if not faithful(text, got[i])]:
            raise ModelRetry(f"items {wrong} changed a {{placeholder}} or a line break; copy each placeholder verbatim and keep every line break a real line break")
        return output

    return translator, Batch


async def run(jobs, batch_size):
    from pydantic_ai.messages import ModelResponse
    from pydantic_ai.models.openrouter import OpenRouterModel

    translator, Batch = agent()
    models = {name: OpenRouterModel(name) for name in {LANGS[job.lang][1] for job in jobs}}
    gate = asyncio.Semaphore(CONCURRENCY)
    spent = 0.0

    async def one(job):
        nonlocal spent
        name = job.out.relative_to(ROOT)
        values = {path: text for path, text in existing(job.out).items() if path in job.leaves}

        def save():
            job.out.parent.mkdir(parents=True, exist_ok=True)
            job.out.write_text(json.dumps(build(job.skeleton, values), ensure_ascii=False, indent=2) + "\n")

        if not job.todo:
            save()
        for start in range(0, len(job.todo), batch_size):
            paths = job.todo[start : start + batch_size]
            items = [{"id": i, "context": f"{job.file}.{'.'.join(p)}", "text": job.leaves[p]} for i, p in enumerate(paths)]
            language, model = LANGS[job.lang]
            deps = Batch(language, {item["id"]: item["text"] for item in items})
            for attempt in range(1, ATTEMPTS + 1):
                try:
                    async with gate:
                        result = await translator.run(
                            json.dumps(items, ensure_ascii=False),
                            model=models[model],
                            deps=deps,
                            model_settings={"openrouter_usage": {"include": True}, "openrouter_reasoning": {"effort": REASONING[model]}},
                        )
                    break
                except Exception as error:
                    status = getattr(error, "status_code", None)
                    # a rejected request (bad key, no credit) fails the same way every time
                    permanent = status is not None and 400 <= status < 500 and status not in (408, 429)
                    if permanent or attempt == ATTEMPTS:
                        print(f"failed {name}: {type(error).__name__}: {str(error)[:300]}", file=sys.stderr)
                        return False
                    await asyncio.sleep(10 * attempt)
            spent += sum((m.provider_details or {}).get("cost", 0) for m in result.all_messages() if isinstance(m, ModelResponse))
            got = {item.id: item.text for item in result.output.items}
            values.update({path: got[i] for i, path in enumerate(paths)})
            # saved after every request, so a stopped run keeps its work; --missing picks up the rest
            save()
        print(f"done {name}", file=sys.stderr)
        return True

    ok = all(await asyncio.gather(*(one(job) for job in jobs)))
    print(f"${spent:.3f} spent", file=sys.stderr)
    return ok


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--lang", metavar="TAG[,TAG...]", help=f"only these languages, of {', '.join(LANGS)}")
    ap.add_argument("--file", metavar="FILE[,FILE...]", help="only these files of locale/en, without .json")
    ap.add_argument("--missing", action="store_true", help="only strings the target file does not have yet")
    ap.add_argument("--batch", type=int, default=100, help="strings per request (default 100)")
    ap.add_argument("--dry-run", action="store_true", help="print what would be sent, send nothing")
    ap.add_argument("--check", action="store_true", help="check the locale files on disk against English, send nothing")
    ap.add_argument("--test", action="store_true", help="run the self-test of the JSON handling")
    ap.add_argument("--folder", type=pathlib.Path, help="another locale folder, such as a zsuite package's")
    args = ap.parse_args()
    if args.folder is not None:
        global LOCALE
        LOCALE = args.folder.resolve()

    if args.test:
        return test()
    if args.check:
        args.missing = False
        bad = report(plan(args))
        print(f"{bad} file(s) with problems" if bad else "ok")
        sys.exit(1 if bad else 0)

    jobs = plan(args)
    if not jobs:
        print("nothing to do")
        return
    langs = list(dict.fromkeys(job.lang for job in jobs))
    print(f"{sum(len(job.todo) for job in jobs)} string(s) in {len(jobs)} file(s), {len(langs)} language(s): {' '.join(langs)}")
    if args.dry_run:
        return
    if not os.environ.get("OPENROUTER_API_KEY"):
        sys.exit("OPENROUTER_API_KEY is not set")

    try:
        ok = asyncio.run(run(jobs, args.batch))
    except KeyboardInterrupt:
        sys.exit("stopped; every finished request is saved, rerun with --missing to translate only the rest")
    bad = report(jobs)
    sys.exit(0 if ok and not bad else 1)


def test():
    skeleton = {"a": "one {n}", "b": {"c": "two", "d": "three"}}
    paths = flatten(skeleton)
    assert paths[("b", "c")] == "two" and len(paths) == 3
    assert build(skeleton, {p: t.upper() for p, t in paths.items()}) == {"a": "ONE {N}", "b": {"c": "TWO", "d": "THREE"}}
    # an untranslated string is left out, a stray key never appears
    assert build(skeleton, {("a",): "één {n}", ("zz",): "stray"}) == {"a": "één {n}"}
    assert build(skeleton, {}) == {}
    assert faithful("a {n}\nb", "c\n{n} d") and not faithful("a {n}", "a") and not faithful("a\nb", "a\\nb")
    print("ok")


if __name__ == "__main__":
    main()
