# Contributing

환영합니다. 작은 PR 도 좋습니다.

## 개발

```bash
cargo test --workspace        # 서버 없이 도는 단위 테스트
cargo clippy --workspace --all-targets
python tools/preview.py blueprints/cottage.txt -o /tmp/c.png   # 미리보기 도구 확인
```

실서버 검증이 필요하면 `online-mode=false`, `gamemode=creative` 로컬 1.21.1 서버를 띄우세요.

## 규칙

- 인증 정보·토큰·서버 주소를 커밋하지 마세요.
- 프로토콜 변경은 패킷 ID 출처(minecraft-data 등)를 PR 설명에 적어 주세요.
- 새 기능에는 서버 없이 돌 수 있는 단위 테스트를 함께 추가해 주세요.
- 남의 서버에서 쓰지 않는 것이 원칙입니다. 반드시 운영자 허락을 받으세요.

## 아이디어

블록 상태(계단 방향 등), 버전별 패킷 ID 표, 상자에서 자재 꺼내기(생존 모드).
