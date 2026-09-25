# `wt` 공개 릴리스 조사

갱신: 2026-09-25. cargo-dist **0.33.0**의 세 타깃 GitHub Release와 셸 설치기를 기준으로 한다.

## 결정

- cargo-dist installer는 `shell`을 사용한다. Homebrew tap, 게시 job, tap token은 사용하지 않는다.
- macOS와 Linux 설치는 cargo-dist가 생성한 `worktree-installer.sh`로 제공한다. 기본 설치 경로는 `CARGO_HOME`이며 설치기는 PATH 추가를 시도한다.
- 셸에서 `wt switch`가 호출 셸의 디렉터리를 바꾸도록 하려면 바이너리 설치 후 `wt config shell install`을 실행한다.
- 셸 설치기가 지원하지 않는 NixOS와 선언적 설치가 필요한 사용자는 기존 Nix flake/Home Manager 경로를 사용한다.

## 검증된 설정

- 대상은 `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`다.
- GitHub Releases를 호스팅으로 사용하고 SHA256 체크섬과 GitHub artifact attestation을 유지한다.
- Attestation은 기본 `build-local-artifacts` job에서 생성한다. 이 job은 `contents: read`, `id-token: write`, `attestations: write` 권한을 가진다. `plan`과 `host`는 릴리즈 생성·게시를 위해 `contents: write`를 가진다.
- runner 매핑은 macOS ARM64 `macos-14`, Linux x86_64 `ubuntu-24.04`, Linux ARM64 `ubuntu-24.04-arm`이다.

## Workflow 규칙

- `release.yml`은 버전 형식과 맞는 태그 push에서만 실행한다. Pull request 검증은 별도 CI workflow가 담당한다.
- 전역 workflow 권한은 `contents: read`다. 각 job은 필요한 권한만 선언하고 checkout은 `persist-credentials: false`를 사용한다.
- 수동 권한·credential hardening을 유지하도록 `allow-dirty = ["ci"]`를 둔다. cargo-dist workflow를 다시 생성할 때는 생성본과 수동 보안 변경을 재검토한다.
- GitHub Actions 참조는 전체 commit SHA로 고정한다.

## 릴리즈 검증

- `cargo dist plan`에서 세 아카이브, 셸 설치기, 통합 `sha256.sum`, attestation 구성을 확인한다.
- 실제 태그 게시 뒤 Release asset을 내려받아 `sha256.sum`을 확인하고 각 아카이브에 `gh attestation verify`를 실행한다.
- macOS ARM64와 Linux x86_64/ARM64에서 태그 고정 설치기를 실행하고 `wt --help`와 `wt config shell install`을 확인한다. NixOS는 flake 경로로 확인한다.

## 참고 자료

- [cargo-dist 0.33.0 설정](https://github.com/axodotdev/cargo-dist/blob/v0.33.0/book/src/reference/config.md)
- [cargo-dist 0.33.0 셸 설치기](https://github.com/axodotdev/cargo-dist/blob/v0.33.0/book/src/installers/shell.md)
- [cargo-dist 0.33.0 체크섬](https://github.com/axodotdev/cargo-dist/blob/v0.33.0/book/src/artifacts/checksums.md)
- [cargo-dist 0.33.0 GitHub attestation](https://github.com/axodotdev/cargo-dist/blob/v0.33.0/book/src/supplychain-security/attestations/github.md)
- [GitHub artifact attestation 검증](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/verify-attestations)
