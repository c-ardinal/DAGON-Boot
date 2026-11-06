"""The one-line install scripts, against a local fake release.

    python -m unittest discover -s scripts -p "test_*.py"

Nothing in the scripts points at a test server: a fake `curl` and `uname` come
first on PATH for install.sh, and a fake Invoke-WebRequest shadows the cmdlet for
install.ps1, so the scripts run as they are and every URL they ask for is seen.
"""
import hashlib
import io
import json
import os
import pathlib
import re
import shutil
import subprocess
import tarfile
import tempfile
import unittest
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
SH = ROOT / "install.sh"
PS1 = ROOT / "install.ps1"
RELEASES = "https://github.com/c-ardinal/DAGON-Boot/releases/download/"
API = "https://api.github.com/repos/c-ardinal/DAGON-Boot/releases?per_page=100&page="
LINUX = "x86_64-unknown-linux-gnu"
WINDOWS = "x86_64-pc-windows-msvc"
# Newest first, as GitHub lists them; other components and a pre-release among them.
TAGS = ["cli-v2.0.0-rc.1", "bootcore-rp2040-v9.9.9", "cli-v1.2.3", "cli-v1.2.2"]

FAKE_CURL = r"""#!/bin/sh
out=""
url=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift ;;
        https://*) url="$1" ;;
    esac
    shift
done
printf '%s\n' "$url" >> "$FAKE_RELEASE/urls.log"
case "$url" in
    https://api.github.com/*)
        page="${url##*page=}"
        file="$FAKE_RELEASE/releases-$page.json"
        if [ ! -f "$file" ] && [ "$page" != 1 ]; then echo '[]'; exit 0; fi
        ;;
    https://github.com/c-ardinal/DAGON-Boot/releases/download/*) file="$FAKE_RELEASE/${url#https://github.com/c-ardinal/DAGON-Boot/releases/download/}" ;;
    *) exit 22 ;;
esac
[ -f "$file" ] || exit 22
if [ -n "$out" ]; then cp "$file" "$out"; else cat "$file"; fi
"""
# Holds a run between staging its file and renaming it, so overlapping runs meet there.
SLOW_MV = """#!/bin/sh
sleep 1
exec "$REAL_MV" "$@"
"""
FAKE_UNAME = """#!/bin/sh
case "$1" in
    -s) echo "$FAKE_SYSTEM" ;;
    -m) echo "$FAKE_MACHINE" ;;
esac
"""
FAKE_IWR = r"""
function Invoke-WebRequest {
    param([string]$Uri, [string]$OutFile, [switch]$UseBasicParsing)
    Add-Content -LiteralPath (Join-Path $env:FAKE_RELEASE 'urls.log') -Value $Uri
    $prefix = 'https://github.com/c-ardinal/DAGON-Boot/releases/download/'
    if ($Uri.StartsWith('https://api.github.com/')) {
        $page = $Uri.Substring($Uri.LastIndexOf('page=') + 5)
        $file = Join-Path $env:FAKE_RELEASE "releases-$page.json"
        if (-not (Test-Path -LiteralPath $file) -and $page -ne '1') { return [pscustomobject]@{ Content = '[]' } }
    }
    elseif ($Uri.StartsWith($prefix)) { $file = Join-Path $env:FAKE_RELEASE $Uri.Substring($prefix.Length) }
    else { throw "unexpected $Uri" }
    if (-not (Test-Path -LiteralPath $file)) { throw "404 $Uri" }
    if ($OutFile) { Copy-Item -LiteralPath $file -Destination $OutFile; return }
    return [pscustomobject]@{ Content = (Get-Content -Raw -LiteralPath $file) }
}
"""


def tar_gz(names):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as t:
        for name in names:
            info = tarfile.TarInfo(name)
            body = f"binary {name}".encode()
            info.size = len(body)
            info.mode = 0o755
            t.addfile(info, io.BytesIO(body))
    return buffer.getvalue()


def a_zip(names):
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as z:
        for name in names:
            z.writestr(name, f"binary {name}")
    return buffer.getvalue()


class FakeRelease:
    def __init__(self, root):
        self.root = pathlib.Path(root)
        self.list_page(1, TAGS)

    def list_page(self, page, tags, indent=2):
        (self.root / f"releases-{page}.json").write_text(json.dumps([{"tag_name": t, "name": t} for t in tags], indent=indent), encoding="utf-8")

    def publish(self, version, target, data, suffix, checksum=None):
        folder = self.root / f"cli-v{version}"
        folder.mkdir(exist_ok=True)
        name = f"dagon-cli-{version}-{target}{suffix}"
        (folder / name).write_bytes(data)
        if checksum != "none":
            digest = checksum or hashlib.sha256(data).hexdigest()
            (folder / f"{name}.sha256").write_text(f"{digest}  {name}\n", encoding="utf-8")

    def urls(self):
        log = self.root / "urls.log"
        return log.read_text(encoding="utf-8").split() if log.exists() else []


def posix_shells():
    return [s for s in dict.fromkeys(shutil.which(n) for n in ("sh", "dash", "bash")) if s]


@unittest.skipUnless(shutil.which("sh"), "no POSIX shell")
class InstallSh(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = pathlib.Path(self.tmp.name)
        (root / "release").mkdir()
        (root / "fake").mkdir()
        for name, text in (("curl", FAKE_CURL), ("uname", FAKE_UNAME)):
            with open(root / "fake" / name, "w", newline="\n", encoding="utf-8") as f:
                f.write(text)
            (root / "fake" / name).chmod(0o755)
        self.release = FakeRelease(root / "release")
        self.fake = root / "fake"
        self.dir = root / "bin"
        self.good = tar_gz(["dagon"])

    def run_script(self, *args, shell="sh", system="Linux", machine="x86_64", env=None, cwd=None):
        environment = {
            **os.environ,
            "PATH": f"{self.fake}{os.pathsep}{os.environ['PATH']}",
            "FAKE_RELEASE": str(self.release.root),
            "FAKE_SYSTEM": system,
            "FAKE_MACHINE": machine,
            "DAGON_INSTALL_DIR": str(self.dir),
            **(env or {}),
        }
        environment.pop("DAGON_VERSION", None) if not (env and "DAGON_VERSION" in env) else None
        # Piped, as the one-liner runs it.
        return subprocess.run([shutil.which(shell), "-s", "--", *args], input=SH.read_bytes(), env=environment, capture_output=True, cwd=cwd)

    def assert_nothing_installed(self, r):
        self.assertNotEqual(r.returncode, 0, r.stdout)
        self.assertFalse(self.dir.exists() and list(self.dir.iterdir()), "the folder stays as it was")

    def test_every_posix_shell_at_hand_installs_the_latest_cli_release(self):
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz")
        for shell in posix_shells():
            with self.subTest(shell=shell):
                r = self.run_script(shell=shell)
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertEqual((self.dir / "dagon").read_bytes(), b"binary dagon")
                self.assertIn(b"dagon 1.2.3", r.stdout)
                shutil.rmtree(self.dir)

    def test_each_system_picks_its_archive(self):
        for system, machine, target in (("Linux", "x86_64", LINUX), ("Darwin", "arm64", "aarch64-apple-darwin"), ("Darwin", "x86_64", "x86_64-apple-darwin")):
            with self.subTest(target=target):
                self.release.publish("1.2.3", target, self.good, ".tar.gz")
                r = self.run_script(system=system, machine=machine)
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertIn(f"{RELEASES}cli-v1.2.3/dagon-cli-1.2.3-{target}.tar.gz", self.release.urls())

    def test_the_user_may_name_a_version(self):
        self.release.publish("1.2.2", LINUX, self.good, ".tar.gz")
        for kwargs in ({"args": ("1.2.2",)}, {"env": {"DAGON_VERSION": "1.2.2"}}):
            with self.subTest(**kwargs):
                r = self.run_script(*kwargs.get("args", ()), env=kwargs.get("env"))
                self.assertEqual(r.returncode, 0, r.stderr)
                self.assertIn(b"dagon 1.2.2", r.stdout)
                self.assertNotIn(API, self.release.urls()[-2:])

    def test_a_stable_cli_release_on_a_later_page_is_found(self):
        self.release.list_page(1, [f"bootcore-rp2040-v{n}.0.0" for n in range(99)] + ["cli-v3.0.0-rc.1"])
        self.release.list_page(2, ["mcu-rp2040-v1.0.0", "cli-v1.2.3", "cli-v1.2.2"])
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz")
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(b"dagon 1.2.3", r.stdout)
        self.assertEqual([u for u in self.release.urls() if u.startswith(API)], [API + "1", API + "2"])

    def test_a_list_without_a_cli_release_stops_when_the_pages_run_out(self):
        self.release.list_page(1, ["bootcore-rp2040-v1.0.0"])
        self.release.list_page(2, ["mcu-rp2040-v1.0.0"])
        r = self.run_script()
        self.assert_nothing_installed(r)
        self.assertIn(b"no release", r.stderr)
        self.assertEqual([u for u in self.release.urls() if u.startswith(API)], [API + "1", API + "2", API + "3"])

    def test_a_list_on_one_line_gives_the_newest_cli_release(self):
        self.release.list_page(1, TAGS, indent=None)
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz")
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(b"dagon 1.2.3", r.stdout)

    def test_overlapping_installs_each_succeed_and_leave_one_whole_file(self):
        with open(self.fake / "mv", "w", newline="\n", encoding="utf-8") as f:
            f.write(SLOW_MV)
        (self.fake / "mv").chmod(0o755)
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz")
        env = {
            **os.environ,
            "PATH": f"{self.fake}{os.pathsep}{os.environ['PATH']}",
            "REAL_MV": shutil.which("mv"),
            "FAKE_RELEASE": str(self.release.root),
            "FAKE_SYSTEM": "Linux",
            "FAKE_MACHINE": "x86_64",
            "DAGON_INSTALL_DIR": str(self.dir),
            "DAGON_VERSION": "1.2.3",
        }
        runs = [subprocess.Popen([shutil.which("sh"), str(SH)], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE) for _ in range(6)]
        results = [(r.communicate(), r.returncode) for r in runs]
        for (_, err), code in results:
            self.assertEqual(code, 0, err)
        self.assertEqual([p.name for p in self.dir.iterdir()], ["dagon"])
        self.assertEqual((self.dir / "dagon").read_bytes(), b"binary dagon")

    def test_a_relative_folder_is_installed_and_reported_whole(self):
        root = pathlib.Path(self.tmp.name)
        work = root / "work"
        work.mkdir()
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz")
        r = self.run_script(env={"DAGON_INSTALL_DIR": "bin"}, cwd=work)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((work / "bin" / "dagon").read_bytes(), b"binary dagon")
        self.assertNotIn(b'export PATH="bin:', r.stdout)
        self.assertNotIn(b"installed in bin", r.stdout)

    def test_a_new_install_replaces_the_old_one(self):
        self.dir.mkdir()
        (self.dir / "dagon").write_bytes(b"old")
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz")
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual([p.name for p in self.dir.iterdir()], ["dagon"])
        self.assertEqual((self.dir / "dagon").read_bytes(), b"binary dagon")

    def test_a_failed_run_removes_its_own_staged_file_and_no_other(self):
        self.dir.mkdir()
        (self.dir / ".dagon.new").write_bytes(b"not this run's")
        with open(self.fake / "chmod", "w", newline="\n", encoding="utf-8") as f:
            f.write("#!/bin/sh\nexit 1\n")
        (self.fake / "chmod").chmod(0o755)
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz")
        self.assertNotEqual(self.run_script().returncode, 0)
        self.assertEqual([p.name for p in self.dir.iterdir()], [".dagon.new"])

    def test_a_wrong_checksum_stops_it(self):
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz", checksum="0" * 64)
        r = self.run_script()
        self.assert_nothing_installed(r)
        self.assertIn(b"does not match", r.stderr)

    def test_a_wrong_checksum_leaves_the_installed_one(self):
        self.dir.mkdir()
        (self.dir / "dagon").write_bytes(b"old")
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz", checksum="0" * 64)
        self.assertNotEqual(self.run_script().returncode, 0)
        self.assertEqual((self.dir / "dagon").read_bytes(), b"old")

    def test_a_missing_or_empty_checksum_file_stops_it(self):
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz", checksum="none")
        self.assert_nothing_installed(self.run_script())
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz", checksum="not-a-sum")
        self.assert_nothing_installed(self.run_script())
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz", checksum="abc123")
        self.assert_nothing_installed(self.run_script())

    def test_an_unknown_system_stops_it_before_any_download(self):
        for system, machine in (("FreeBSD", "amd64"), ("Linux", "aarch64"), ("Linux", "riscv64"), ("Darwin", "ppc")):
            with self.subTest(system=system, machine=machine):
                r = self.run_script(system=system, machine=machine)
                self.assert_nothing_installed(r)
                self.assertIn(b"no release for", r.stderr)
                self.assertEqual(self.release.urls(), [])

    def test_a_failed_download_stops_it(self):
        self.assert_nothing_installed(self.run_script("9.9.9"))

    def test_an_unreadable_list_of_releases_stops_it(self):
        (self.release.root / "releases-1.json").unlink()
        self.assert_nothing_installed(self.run_script())

    def test_a_version_that_is_not_one_stops_it(self):
        for bad in ("../../evil", "1.2.3/../x", "$(touch x)", "v1.2.3"):
            with self.subTest(bad=bad):
                self.assert_nothing_installed(self.run_script(bad))
                self.assertEqual(self.release.urls(), [])

    def test_an_archive_without_dagon_stops_it(self):
        self.release.publish("1.2.3", LINUX, tar_gz(["other"]), ".tar.gz")
        self.assert_nothing_installed(self.run_script())

    def test_it_says_how_to_add_the_folder_to_path(self):
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz")
        r = self.run_script()
        self.assertIn(b"export PATH=", r.stdout)

    def test_every_url_is_https_and_curl_is_held_to_it(self):
        self.release.publish("1.2.3", LINUX, self.good, ".tar.gz")
        self.run_script()
        self.assertTrue(self.release.urls())
        self.assertTrue(all(u.startswith("https://") for u in self.release.urls()))
        text = SH.read_text(encoding="utf-8")
        self.assertIn("--proto '=https'", text)
        self.assertEqual(re.findall(r"http://", text), [])


def powershell():
    return shutil.which("powershell") or shutil.which("pwsh")


@unittest.skipUnless(powershell() and os.name == "nt", "no Windows PowerShell")
class InstallPs1(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = pathlib.Path(self.tmp.name)
        (root / "release").mkdir()
        self.release = FakeRelease(root / "release")
        self.dir = root / "bin"
        self.harness = root / "harness.ps1"
        # Through Invoke-Expression, as the one-liner runs it.
        self.harness.write_text(FAKE_IWR + f"\nGet-Content -Raw -LiteralPath '{PS1}' | Invoke-Expression\n", encoding="utf-8")
        self.good = a_zip(["dagon.exe"])

    def run_script(self, machine="AMD64", env=None, cwd=None):
        environment = {
            **os.environ,
            "FAKE_RELEASE": str(self.release.root),
            "PROCESSOR_ARCHITECTURE": machine,
            "DAGON_INSTALL_DIR": str(self.dir),
            # Never the real user's PATH.
            "DAGON_NO_MODIFY_PATH": "1",
            **(env or {}),
        }
        environment.pop("PROCESSOR_ARCHITEW6432", None)
        if not (env and "DAGON_VERSION" in env):
            environment.pop("DAGON_VERSION", None)
        return subprocess.run([powershell(), "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(self.harness)], env=environment, capture_output=True, cwd=cwd)

    def assert_nothing_installed(self, r):
        self.assertNotEqual(r.returncode, 0, r.stdout)
        self.assertFalse(self.dir.exists() and list(self.dir.iterdir()), "the folder stays as it was")

    def test_it_installs_the_latest_cli_release(self):
        self.release.publish("1.2.3", WINDOWS, self.good, ".zip")
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual((self.dir / "dagon.exe").read_bytes(), b"binary dagon.exe")
        self.assertIn(b"dagon 1.2.3", r.stdout)
        self.assertIn(b"not on your PATH", r.stdout)
        self.assertTrue(all(u.startswith("https://") for u in self.release.urls()))

    def test_the_user_may_name_a_version(self):
        self.release.publish("1.2.2", WINDOWS, self.good, ".zip")
        r = self.run_script(env={"DAGON_VERSION": "1.2.2"})
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(b"dagon 1.2.2", r.stdout)
        self.assertNotIn(API, self.release.urls())

    def test_a_stable_cli_release_on_a_later_page_is_found(self):
        self.release.list_page(1, [f"bootcore-rp2040-v{n}.0.0" for n in range(99)] + ["cli-v3.0.0-rc.1"])
        self.release.list_page(2, ["mcu-rp2040-v1.0.0", "cli-v1.2.3", "cli-v1.2.2"])
        self.release.publish("1.2.3", WINDOWS, self.good, ".zip")
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(b"dagon 1.2.3", r.stdout)
        self.assertEqual([u for u in self.release.urls() if u.startswith(API)], [API + "1", API + "2"])

    def test_a_relative_folder_follows_powershells_location(self):
        # .NET resolves a relative path against the process's folder, here A.
        root = pathlib.Path(self.tmp.name)
        a, b = root / "A", root / "B"
        (a / "bin").mkdir(parents=True)
        (b / "bin").mkdir(parents=True)
        self.harness.write_text(f"Set-Location -LiteralPath '{b}'\n" + self.harness.read_text(encoding="utf-8"), encoding="utf-8")
        self.release.publish("1.2.3", WINDOWS, self.good, ".zip")
        r = self.run_script(env={"DAGON_INSTALL_DIR": ".\\bin"}, cwd=a)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(list((a / "bin").iterdir()), [], "nothing in the process's folder")
        self.assertEqual([p.name for p in (b / "bin").iterdir()], ["dagon.exe"])
        self.assertEqual((b / "bin" / "dagon.exe").read_bytes(), b"binary dagon.exe")
        self.assertIn(str(b / "bin").encode(), r.stdout.replace(b"\r\n", b""))

    def test_a_new_install_replaces_the_old_one(self):
        self.dir.mkdir()
        (self.dir / "dagon.exe").write_bytes(b"old")
        self.release.publish("1.2.3", WINDOWS, self.good, ".zip")
        r = self.run_script()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual([p.name for p in self.dir.iterdir()], ["dagon.exe"])
        self.assertEqual((self.dir / "dagon.exe").read_bytes(), b"binary dagon.exe")

    def test_a_list_without_a_cli_release_stops_when_the_pages_run_out(self):
        self.release.list_page(1, ["bootcore-rp2040-v1.0.0"])
        r = self.run_script()
        self.assert_nothing_installed(r)
        self.assertEqual([u for u in self.release.urls() if u.startswith(API)], [API + "1", API + "2"])

    def test_overlapping_installs_each_succeed_and_leave_one_whole_file(self):
        # Held after Copy-Item, so overlapping runs meet between staging and the rename.
        text = self.harness.read_text(encoding="utf-8")
        slow = "function Copy-Item { Microsoft.PowerShell.Management\\Copy-Item @args; Start-Sleep -Milliseconds 1500 }\n"
        self.harness.write_text(slow + text, encoding="utf-8")
        self.release.publish("1.2.3", WINDOWS, self.good, ".zip")
        env = {**os.environ, "FAKE_RELEASE": str(self.release.root), "PROCESSOR_ARCHITECTURE": "AMD64", "DAGON_INSTALL_DIR": str(self.dir),
               "DAGON_NO_MODIFY_PATH": "1", "DAGON_VERSION": "1.2.3"}
        env.pop("PROCESSOR_ARCHITEW6432", None)
        runs = [subprocess.Popen([powershell(), "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(self.harness)], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE) for _ in range(4)]
        results = [(r.communicate(), r.returncode) for r in runs]
        for (_, err), code in results:
            self.assertEqual(code, 0, err)
        self.assertEqual([p.name for p in self.dir.iterdir()], ["dagon.exe"])
        self.assertEqual((self.dir / "dagon.exe").read_bytes(), b"binary dagon.exe")

    def test_a_wrong_checksum_stops_it(self):
        self.release.publish("1.2.3", WINDOWS, self.good, ".zip", checksum="0" * 64)
        r = self.run_script()
        self.assert_nothing_installed(r)
        self.assertIn(b"does not match", r.stderr)

    def test_a_missing_or_empty_checksum_file_stops_it(self):
        self.release.publish("1.2.3", WINDOWS, self.good, ".zip", checksum="none")
        self.assert_nothing_installed(self.run_script())
        self.release.publish("1.2.3", WINDOWS, self.good, ".zip", checksum="not-a-sum")
        self.assert_nothing_installed(self.run_script())
        self.release.publish("1.2.3", WINDOWS, self.good, ".zip", checksum="abc123")
        self.assert_nothing_installed(self.run_script())

    def test_an_unknown_system_stops_it_before_any_download(self):
        for machine in ("ARM64", "x86"):
            with self.subTest(machine=machine):
                r = self.run_script(machine=machine)
                self.assert_nothing_installed(r)
                self.assertIn(b"no release for", r.stderr)
                self.assertEqual(self.release.urls(), [])

    def test_a_version_that_is_not_one_stops_it(self):
        for bad in ("..\\..\\evil", "1.2.3/../x", "v1.2.3"):
            with self.subTest(bad=bad):
                self.assert_nothing_installed(self.run_script(env={"DAGON_VERSION": bad}))
                self.assertEqual(self.release.urls(), [])

    def test_an_archive_without_dagon_stops_it(self):
        self.release.publish("1.2.3", WINDOWS, a_zip(["other"]), ".zip")
        self.assert_nothing_installed(self.run_script())


class Scripts(unittest.TestCase):
    def test_the_scripts_know_exactly_the_targets_the_release_builds(self):
        release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        built = set(re.findall(r"target: ([\w-]+)", release))
        named = set(re.findall(r'"([\w]+-(?:unknown-linux-gnu|apple-darwin))"', SH.read_text(encoding="utf-8")))
        named |= set(re.findall(r"'([\w]+-pc-windows-msvc)'", PS1.read_text(encoding="utf-8")))
        self.assertEqual(named, built)

    def test_the_scripts_read_what_the_release_publishes(self):
        made = (ROOT / "scripts/release.py").read_text(encoding="utf-8")
        self.assertIn("dagon-cli-{version}-{target}{'.zip' if windows else '.tar.gz'}", made)
        self.assertIn('archive.name + ".sha256"', made)
        self.assertIn("dagon-cli-$version-$target.tar.gz", SH.read_text(encoding="utf-8"))
        self.assertIn("dagon-cli-$version-$target.zip", PS1.read_text(encoding="utf-8"))
        for script in (SH, PS1):
            text = script.read_text(encoding="utf-8")
            self.assertIn("/releases/download/cli-v$version/", text, script.name)
            self.assertIn(".sha256", text, script.name)

    def test_only_a_cli_tag_publishes_and_only_after_every_build(self):
        release = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        jobs = dict(re.findall(r"(?ms)^  ([a-z0-9-]+):\n(.*?)(?=^  [a-z0-9-]+:\n|\Z)", release.split("\njobs:\n", 1)[1]))
        self.assertEqual(release.split("\njobs:\n", 1)[0].split("permissions:", 1)[1].split(), ["contents:", "read"])
        publish = jobs["publish"]
        self.assertIn("needs: build\n", publish)
        self.assertIn("if: startsWith(github.ref, 'refs/tags/cli-v')\n", publish)
        self.assertIn("contents: write\n", publish)
        self.assertIn('gh release create "$TAG" out/release/*', publish)
        self.assertEqual(re.findall(r"uses: (?!actions/)\S+", publish), [])
        for name, job in jobs.items():
            if name != "publish":
                self.assertNotIn("contents: write", job, name)
                self.assertNotIn("gh release", job, name)

    def test_no_script_elevates_or_runs_what_it_downloads(self):
        self.assertEqual(re.findall(r"(?m)^[^#]*\b(sudo|eval)\b", SH.read_text(encoding="utf-8")), [])
        self.assertEqual(re.findall(r"(?mi)^[^#]*\b(RunAs|Invoke-Expression|iex)\b", PS1.read_text(encoding="utf-8")), [])

    def test_the_powershell_script_is_ascii_for_windows_powershell_5_1(self):
        self.assertTrue(PS1.read_bytes().isascii())

    def test_the_powershell_script_uses_nothing_newer_than_5_1(self):
        text = "\n".join(l for l in PS1.read_text(encoding="utf-8").splitlines() if not l.lstrip().startswith("#"))
        for newer in ("??", "?.", " && ", " || ", "-SkipHttpErrorCheck", "ForEach-Object -Parallel"):
            self.assertNotIn(newer, text, newer)

    def test_the_one_liners_point_at_the_main_branch(self):
        for rel in ("README.md", "README.en.md", "site/src/content/docs/getting-started/install.md", "site/src/content/docs/en/getting-started/install.md"):
            text = (ROOT / rel).read_text(encoding="utf-8")
            self.assertIn("curl -fsSL https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/install.sh | sh", text, rel)
            self.assertIn("irm https://raw.githubusercontent.com/c-ardinal/DAGON-Boot/main/install.ps1 | iex", text, rel)


if __name__ == "__main__":
    unittest.main()
