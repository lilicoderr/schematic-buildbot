# Changelog

## [Unreleased]

### Added
- `.litematic` 파서(`litematic.rs`): 다중 리전, 비트 패킹 BlockStates, 음수 Size 정규화.
- `--dry-run` 이 층별 ASCII 도면과 공중(받침 없는) 블록 경고를 출력한다.
- `--delay-ms <N>` 설치 간 대기 설정 (패킷 레이트 제한이 엄격한 서버용).
- `tools/preview.py`: 설계도(.txt/.schem) → 아이소메트릭 PNG (표준 라이브러리만). CI 스모크 테스트 포함.
- `Schematic::ascii_layers`, `Schematic::floating_blocks` 와 테스트.

### Changed
- README 를 기능 표·동작 원리 다이어그램 중심으로 재작성.
- clippy 경고 정리(`io::Error::other`, 테스트 상수).

## 0.1.0

- 1.21.1(767) 프로토콜 직접 구현: 핸드셰이크·로그인·Configuration·Play.
- `.schem`(Sponge v2/v3) 및 URL 직링크 건축, 블록 1333종.
- 지면 이동 기반 건축으로 anti-cheat 킥 없음.
