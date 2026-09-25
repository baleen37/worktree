# `wt` 공개 릴리스

갱신: 2026-09-25. GitHub의 `v0.1.1`은 Cargo 패키지 `worktree`로 이미 공개됐다. 이 변경은 Cargo 패키지를 `worktree-cli`로 바꾸고 다음 버전을 `0.1.2`로 설정한다. `v0.1.0`은 철회했으므로 재사용하지 않는다.

## 배포 경로

- crates.io: `cargo install worktree-cli --locked`
- macOS와 Linux: cargo-dist 셸 설치기 `worktree-cli-installer.sh`
- NixOS와 선언적 설치: Flake 참조 `github:baleen37/worktree/v0.1.2`
- 실행 파일 이름은 `wt`를 유지한다. Cargo library 이름은 기존 Rust 코드와의 호환을 위해 `worktree`로 유지한다.
- cargo-dist 아카이브 접두사는 `worktree-cli-`가 된다. 대상은 `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`다.

Linux 바이너리는 glibc 2.35 이상을 요구한다. Linux 타깃은 Ubuntu 22.04 runner에서 빌드해 Debian 12에서 실행 가능하게 한다. GitHub Release에는 셸 설치기, SHA256 체크섬, artifact attestation을 포함한다.

## crates.io 게시

태그 릴리스에서 `release.yml`은 세 플랫폼의 빌드가 끝난 다음 `cargo publish --dry-run --locked`를 실행한다. crates.io에 같은 버전이 있으면 yank 여부와 package archive의 registry checksum을 확인한다. yank된 버전이거나 checksum이 다르면 GitHub Release 생성을 중단한다. 둘 다 문제가 없으면 중복 게시를 건너뛰고, 버전이 없으면 Trusted Publishing으로 게시한 뒤 GitHub Release를 만든다.

crates.io Trusted Publisher는 첫 수동 게시 뒤에 등록해야 한다. 최초 `worktree-cli 0.1.2`는 해당 버전의 최종 릴리스 커밋에서 `cargo publish --locked`로 한 번 수동 게시한다. 이어서 crates.io에서 Trusted Publisher를 등록하고, 동일 커밋의 `v0.1.2` 태그를 푸시한다. 이후 버전은 GitHub Actions OIDC로 게시한다. 장기 API token은 GitHub Secrets에 저장하지 않는다.

Trusted Publisher 등록값:

- Repository owner: `baleen37`
- Repository name: `worktree`
- Workflow filename: `release.yml`
- GitHub Actions environment: `crates-io`

## 검증

- `cargo dist plan`에서 `worktree-cli-` 아카이브 3개, `worktree-cli-installer.sh`, `sha256.sum`, attestation 설정을 확인한다.
- Rust, 셸 시나리오, 세 Nix system 빌드와 `cargo publish --dry-run --locked`를 통과시킨다.
- 게시 뒤 Release asset의 체크섬과 각 아카이브 attestation을 확인한다.
- macOS ARM64와 Linux x86_64/ARM64에서 태그 고정 설치기로 `wt --help`를 확인한다. Nix는 `nix profile install github:baleen37/worktree/v0.1.2`로 확인한다.

## 롤백

`v0.1.0`은 Linux 바이너리가 glibc 2.39를 요구해 Debian 12에서 실행되지 않아 철회했다. 태그를 재사용하지 않고 Ubuntu 22.04 runner와 `v0.1.1`로 수정해 현재 GitHub Release를 만들었다.

`worktree-cli 0.1.2` 게시 뒤 검증에 실패하면 해당 crate를 yank하고 해당 GitHub Release와 태그를 제거한다. 수정 버전은 `0.1.3`으로 올린다. Yank는 기존 lockfile에서 해당 버전을 사용하는 일을 막지 않으며, 이미 다운로드된 GitHub asset도 회수하지 못한다.

## 참고 자료

- [crates.io Trusted Publishing](https://crates.io/docs/trusted-publishing)
- [Trusted Publishing bootstrap 절차](https://github.com/rust-lang/rfcs/blob/master/text/3691-trusted-publishing-cratesio.md)
- [Cargo 게시와 yank 정책](https://doc.rust-lang.org/cargo/reference/publishing.html)
- [cargo-dist 0.33.0 설정](https://github.com/axodotdev/cargo-dist/blob/v0.33.0/book/src/reference/config.md)
- [cargo-dist 0.33.0 셸 설치기](https://github.com/axodotdev/cargo-dist/blob/v0.33.0/book/src/installers/shell.md)
- [GitHub artifact attestation 검증](https://docs.github.com/en/actions/how-tos/secure-your-work/use-artifact-attestations/verify-attestations)
