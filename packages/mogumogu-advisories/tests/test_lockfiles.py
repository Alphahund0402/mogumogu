"""Unit tests for static lockfile parsers and symlink-safe directory discovery."""
import json
from pathlib import Path
import tempfile
import unittest

from mogumogu_advisories import Package, discover_lockfiles, parse_lockfile, parse_package_list, scan_lockfiles


class LockfileParserTests(unittest.TestCase):
    def test_parses_all_supported_ecosystem_lockfiles_and_skips_private_or_local(self):
        fixtures = {
            "Cargo.lock": (
                'version = 4\n'
                '[[package]]\nname = "app"\nversion = "0.1.0"\n'
                '[[package]]\nname = "serde_json"\nversion = "1.0.140"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n',
                {Package("crates.io", "serde-json", "1.0.140")},
                1,
            ),
            "package-lock.json": (
                json.dumps({
                    "lockfileVersion": 3,
                    "packages": {
                        "": {"name": "web", "version": "1.0.0"},
                        "node_modules/lodash": {
                            "version": "4.17.21",
                            "resolved": "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz",
                        },
                        "node_modules/@corp/private": {
                            "version": "1.0.0",
                            "resolved": "https://npm.corp.example/private.tgz",
                        },
                        "node_modules/local-link": {"version": "1.0.0", "link": True},
                    },
                }),
                {Package("npm", "lodash", "4.17.21")},
                2,
            ),
            "pnpm-lock.yaml": (
                "lockfileVersion: '9.0'\n"
                "packages:\n"
                "  '@scope/ui@1.2.3':\n"
                "    resolution: {integrity: sha512-abc}\n"
                "  /zod@3.23.8(typescript@5.0.0):\n"
                "    resolution: {integrity: sha512-def}\n"
                "  file:../local-pkg:\n"
                "    resolution: {directory: ../local-pkg}\n",
                {Package("npm", "@scope/ui", "1.2.3"), Package("npm", "zod", "3.23.8")},
                1,
            ),
            "uv.lock": (
                'version = 1\n'
                '[[package]]\nname = "Requests"\nversion = "2.32.3"\n'
                'source = { registry = "https://pypi.org/simple" }\n'
                '[[package]]\nname = "internal"\nversion = "0.1.0"\n'
                'source = { editable = "." }\n',
                {Package("PyPI", "requests", "2.32.3")},
                1,
            ),
            "poetry.lock": (
                '[[package]]\nname = "Flask"\nversion = "3.0.3"\n'
                '[[package]]\nname = "local"\nversion = "0.1.0"\n'
                '[package.source]\ntype = "directory"\nurl = "../local"\n',
                {Package("PyPI", "flask", "3.0.3")},
                1,
            ),
            "requirements.txt": (
                "# comment\n"
                "Django==5.1.2\n"
                "urllib3>=2.0.0\n"
                "-r other.txt\n",
                {Package("PyPI", "django", "5.1.2")},
                2,
            ),
            "go.mod": (
                "module example.com/app\n\n"
                "require (\n"
                "    github.com/gin-gonic/gin v1.10.0\n"
                "    example.com/local v0.0.0\n"
                ")\n\n"
                "replace example.com/local => ../local\n",
                {Package("Go", "github.com/gin-gonic/gin", "1.10.0")},
                1,
            ),
            "packages.lock.json": (
                json.dumps({
                    "version": 1,
                    "dependencies": {
                        "net8.0": {
                            "Newtonsoft.Json": {"type": "Direct", "resolved": "13.0.3"},
                            "My.Project": {"type": "Project", "resolved": "1.0.0"},
                        }
                    },
                }),
                {Package("NuGet", "newtonsoft.json", "13.0.3")},
                1,
            ),
            "gradle.lockfile": (
                "# Gradle lockfile\n"
                "com.google.guava:guava:33.2.1-jre=compileClasspath\n"
                "empty=testRuntimeClasspath\n",
                {Package("Maven", "com.google.guava:guava", "33.2.1-jre")},
                0,
            ),
            "composer.lock": (
                json.dumps({
                    "packages": [
                        {
                            "name": "Monolog/Monolog",
                            "version": "v3.5.0",
                            "notification-url": "https://packagist.org/downloads/",
                        },
                        {
                            "name": "acme/private",
                            "version": "1.0.0",
                            "dist": {"type": "zip", "url": "https://satis.corp.example/a.zip"},
                        },
                        {
                            "name": "acme/branch",
                            "version": "dev-main",
                            "notification-url": "https://packagist.org/downloads/",
                        },
                    ],
                    "packages-dev": [],
                }),
                {Package("Packagist", "monolog/monolog", "3.5.0")},
                2,
            ),
            "Gemfile.lock": (
                "GEM\n"
                "  remote: https://rubygems.org/\n"
                "  specs:\n"
                "    RedCloth (4.3.4)\n"
                "    nokogiri (1.16.5-x86_64-linux)\n"
                "GIT\n"
                "  remote: https://github.com/org/private.git\n"
                "  specs:\n"
                "    private_gem (0.1.0)\n",
                {Package("RubyGems", "RedCloth", "4.3.4"), Package("RubyGems", "nokogiri", "1.16.5")},
                1,
            ),
            "pubspec.lock": (
                "packages:\n"
                "  http:\n"
                "    dependency: \"direct main\"\n"
                "    description:\n"
                "      name: http\n"
                "      url: \"https://pub.dev\"\n"
                "    source: hosted\n"
                "    version: \"1.2.1\"\n"
                "  flutter:\n"
                "    dependency: \"direct main\"\n"
                "    description: flutter\n"
                "    source: sdk\n"
                "    version: \"0.0.0\"\n",
                {Package("Pub", "http", "1.2.1")},
                1,
            ),
            "mix.lock": (
                '%{\n'
                '  "phoenix": {:hex, :phoenix, "1.7.10", "aaa", [:mix], [], "hexpm", "bbb"},\n'
                '  "private": {:hex, :private, "1.0.0", "aaa", [:mix], [], "hexpm:acme", "bbb"},\n'
                '  "gitdep": {:git, "https://github.com/a/b.git", "deadbeef", []},\n'
                '}\n',
                {Package("Hex", "phoenix", "1.7.10")},
                2,
            ),
            "renv.lock": (
                json.dumps({
                    "R": {"Repositories": [{"Name": "CRAN", "URL": "https://cloud.r-project.org"}]},
                    "Packages": {
                        "Rcpp": {"Package": "Rcpp", "Version": "1.0-12", "Source": "Repository", "Repository": "CRAN"},
                        "Biobase": {"Package": "Biobase", "Version": "2.66.0", "Source": "Bioconductor"},
                    },
                }),
                {Package("CRAN", "Rcpp", "1.0-12")},
                1,
            ),
            "Manifest.toml": (
                'manifest_format = "2.1"\n'
                '[registries.General]\nuuid = "23338594-aafe-5451-b93e-139f81909106"\n'
                '[registries.Corp]\nuuid = "11111111-1111-1111-1111-111111111111"\n'
                '[[deps.HTTP]]\ngit-tree-sha1 = "abc"\nregistries = "General"\nuuid = "cd3eb016-35fb-5094-929b-558a96fad6f3"\nversion = "1.10.5"\n'
                '[[deps.PrivatePkg]]\ngit-tree-sha1 = "def"\nregistries = "Corp"\nuuid = "22222222-2222-2222-2222-222222222222"\nversion = "0.2.0"\n'
                '[[deps.Dates]]\nuuid = "ade2ca70-3891-5945-98fb-dc099432e06a"\n',
                {Package("Julia", "HTTP", "1.10.5")},
                1,
            ),
        }

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for filename, (content, expected_pkgs, expected_skipped) in fixtures.items():
                with self.subTest(lockfile=filename):
                    path = root / filename
                    path.write_text(content, encoding="utf-8")
                    scanned = parse_lockfile(path, base_dir=root)
                    self.assertEqual(scanned.packages, expected_pkgs)
                    self.assertEqual(scanned.skipped, expected_skipped)
                    for pkg in expected_pkgs:
                        self.assertEqual(scanned.locations[pkg], {filename})

    def test_discover_and_scan_skips_vendor_node_modules_and_git(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / "Cargo.lock").write_text(
                '[[package]]\nname = "serde"\nversion = "1.0.219"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n',
                encoding="utf-8",
            )
            (root / "web").mkdir()
            (root / "web" / "package-lock.json").write_text(
                json.dumps({"packages": {"node_modules/chalk": {"version": "5.3.0"}}}),
                encoding="utf-8",
            )
            # Noise directories must be skipped.
            for ignored in ("node_modules/sub", "target/debug", ".git/hooks", "vendor/pkg"):
                folder = root / ignored
                folder.mkdir(parents=True)
                (folder / "package-lock.json").write_text("{}", encoding="utf-8")

            discovered = [p.relative_to(root).as_posix() for p in discover_lockfiles(root)]
            self.assertEqual(discovered, ["Cargo.lock", "web/package-lock.json"])

            scanned = scan_lockfiles(root)
            self.assertEqual(scanned.packages, {
                Package("crates.io", "serde", "1.0.219"),
                Package("npm", "chalk", "5.3.0"),
            })
            self.assertEqual(scanned.locations[Package("npm", "chalk", "5.3.0")], {"web/package-lock.json"})

    def test_parse_explicit_package_list(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "packages.json"
            path.write_text(
                json.dumps([
                    {"ecosystem": "Go", "name": "stdlib", "version": "1.24.1"},
                    {"ecosystem": "npm", "name": "bad", "version": "1.x"},
                ]),
                encoding="utf-8",
            )
            scanned = parse_package_list(path)
        self.assertEqual(scanned.packages, {Package("Go", "stdlib", "1.24.1")})
        self.assertEqual(scanned.skipped, 1)


if __name__ == "__main__":
    unittest.main()
