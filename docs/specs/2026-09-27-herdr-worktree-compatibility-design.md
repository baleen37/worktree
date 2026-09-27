# Herdr worktree 수명주기 호환성

Discovery: full

## 목표

`worktree-cli`는 Herdr 0.9.1 이상에서 worktree를 만들고 열고 제거하는 동작을 Herdr workspace 상태와 일치시킨다. Herdr 환경 밖에서는 현재 Git 동작을 유지한다.

## 결정된 범위

- Herdr 통합은 `HERDR_ENV=1`이고 `HERDR_WORKSPACE_ID`가 비어 있지 않을 때 활성화한다. 그 외에는 기존 Git 경로를 사용한다.
- `wt switch`와 `wt switch -c`의 기존 Herdr 생성·열기 동작을 유지한다.
- Herdr가 연 worktree를 제거할 때 Herdr CLI를 호출한다. Herdr가 열지 않은 worktree에는 기존 Git 삭제 경로를 사용한다.
- `.worktrees` 경로, Git merge 동작, 브랜치 삭제 조건, prune 대상 판정 및 확인 절차를 유지한다.
- Herdr 명령이 실패하면 해당 작업을 실패로 보고하고 Git 삭제로 우회하지 않는다.
- 제거 뒤 Herdr 포커스를 `wt`가 선택한 결과 경로와 맞춘다.
- `wt list`는 Git worktree 목록을 계속 표시한다. Herdr 상태 열은 추가하지 않는다.

## 설계

이 명세에서 `dirty`는 `git status --porcelain --untracked-files=all` 출력이 비어 있지 않은 상태를 뜻한다.

### Herdr workspace 확인

활성 Herdr 통합은 `herdr worktree list --workspace <id>`를 사용한다. 응답의 `result.source.source_workspace_id`로 저장소의 부모 workspace ID를 찾고, `result.source.repo_root`가 Git primary root와 같은지 확인한다. `result.worktrees[]`의 `path`와 `open_workspace_id`로 각 열린 자식 worktree를 연결한다. 자식 경로와 `open_workspace_id`가 모두 확인될 때만 그 자식을 Herdr 관리 worktree로 간주한다.

활성 통합에서 `herdr worktree list` 실행, 필수 필드 해석, 또는 저장소 경로 확인이 실패하면 작업을 실패시킨다. 저장소 경로가 다르면 다른 Herdr workspace 아래에 worktree를 만들거나 삭제하지 않는다. 유효한 같은 저장소 응답에 대상 경로가 없거나 해당 행에 `open_workspace_id`가 없으면 Herdr 자식 workspace가 없는 것으로 보고 기존 Git worktree 삭제 경로를 사용한다.

### 명령별 동작

| 명령 | Herdr 활성 상태 | 기존 동작 유지 |
| --- | --- | --- |
| `wt switch <branch>` | 등록된 경로는 `herdr worktree open --focus`; 미등록 경로는 `.worktrees` 아래에 만든 뒤 `herdr worktree create --path ... --focus` | Git branch 확인, remote tracking, 출력 경로 및 shell 경로 변경 |
| `wt switch -c <branch>` | `.worktrees` 경로를 넘겨 `herdr worktree create --focus` | fetch, base fast-forward, dirty base 거부, 브랜치 생성 규칙 |
| `wt remove` | 대상에 열린 Herdr workspace ID가 있으면 `herdr worktree remove --workspace <id>` 사용 | dirty, primary, base worktree 보호 및 기존 merged branch 삭제 조건 |
| `wt merge` | 기존 Git merge 성공 뒤 Herdr로 source checkout을 제거하고 target에 포커스 | source와 target의 working tree 변경 없음 확인, 일반 Git merge, merge 성공 뒤 source branch 삭제 |
| `wt prune` | 확인 및 매 삭제 전 재평가 뒤 열린 Herdr child는 Herdr로 제거하고 나머지는 Git으로 제거; 완료 후 호출자의 경로에 포커스 | 기본 3일 기준, `--all`, branch 보존, 비대화형 미리보기 |

일반 `wt remove`와 `wt merge`는 `herdr worktree remove`에 `--force`를 전달하지 않는다. `wt prune --all`만 dirty와 locked worktree를 포함하도록 Herdr와 Git 삭제에 `--force`를 전달한다. Herdr 삭제가 실패하면 Git 삭제로 재시도하지 않는다.

Herdr 삭제가 성공한 뒤 기존 `wt` 브랜치 정책을 실행한다. 따라서 Herdr의 `worktree remove` 자체는 브랜치를 지우지 않는다. `wt merge`의 Git merge가 성공했으나 Herdr 삭제가 실패하면 명령은 실패를 반환하고, branch cleanup을 진행하지 않는다.

`wt remove`에서 현재 worktree를 제거하면 base 경로를 Herdr 포커스로 선택한다. 다른 경로를 명시해 제거하면 호출자의 현재 경로 포커스를 복원한다. `wt merge`는 merge target으로 포커스를 이동한다. `wt prune`은 기존 보호 규칙에 따라 현재 worktree를 제거하지 않고 완료 뒤 그 경로에 포커스를 복원한다.

### 비목표

- `.worktrees` 대신 Herdr의 기본 worktree 디렉터리를 사용하지 않는다.
- Herdr 소켓 API를 직접 구현하지 않는다.
- Git 외부에서 시작한 worktree를 목록에 추가하거나 `wt list` 출력 형식을 바꾸지 않는다.
- dirty worktree 강제 삭제, branch 삭제 규칙 변경, merge 방식 변경을 추가하지 않는다.

## 검증 기준

### 계약 테스트

- 현재 Herdr JSON 응답에서 저장소 부모 ID와 열린 child `open_workspace_id`를 추출한다.
- `switch`, `switch -c`, `remove`, `merge`, `prune`이 각기 정확한 Herdr 인자와 경로를 사용한다.
- Herdr workspace ID가 없는 대상은 기존 Git 삭제 경로를 사용하고 브랜치 정책을 유지한다.
- Herdr 호출이나 응답 파싱이 실패하면 Git 삭제로 우회하지 않는다.
- 다른 repository root를 반환하는 Herdr workspace ID는 거부하고 Git 삭제로 우회하지 않는다.
- 현재 worktree 제거, merge, prune 뒤 기대한 경로와 일치하는 Herdr 포커스를 확인한다.
- Herdr 비활성 상태의 기존 Git 동작과 `.worktrees` 위치를 회귀 확인한다.

### 설치 Herdr 검증

Herdr 0.9.1이 제공하는 CLI를 별도의 이름 있는 session과 임시 Git 저장소에서 사용한다. 생성, 기존 worktree 열기, 현재 worktree 제거, merge source 정리, prune을 실행한다. 각 작업 뒤 `herdr worktree list`와 `git worktree list --porcelain`을 비교하고, branch 보존/삭제 및 Herdr 포커스를 확인한다. 기본 Herdr session과 저장소 worktree에는 변경을 가하지 않는다.

Herdr CLI 계약: [CLI reference](https://github.com/herdrdev/herdr/blob/master/docs/next/website/src/content/docs/cli-reference.mdx#worktrees). 이 설계의 설치 기준은 Herdr 0.9.1이다.

## Open questions
