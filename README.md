# schematic-buildbot

**Rust** 로 Minecraft **1.21.1** 프로토콜(767)을 직접 구현한 — 라이브러리 없이 — 봇.
mineflayer/azalea 같은 기성 봇 프레임워크를 쓰지 않고, 핸드셰이크·로그인·설정·플레이 패킷을 손으로 파싱한다.

두 가지를 한다.

1. **packet-probe** — 서버에 붙어 프로토콜을 "리버스"로 관찰한다. keepalive·텔레포트에 정확히 응답해 **킥당하지 않고** Play 패킷 흐름을 수집·분류한다.
2. **build-bot** — 설계도(blueprint)대로 블록을 짓는다. **지면을 걸어 다니며**(공중부양·과속 없이) 설치해 이동 검증(anti-cheat)에 걸리지 않는다.

> 대상은 `online-mode=false`(오프라인) 서버. 온라인 모드는 Mojang 인증/암호화가 필요해 범위를 벗어난다.
> 실제 1.21.1 바닐라 서버에서 로그인→유지→건축을 검증했다.

## 왜 손으로 구현했나

- 요구가 "프로토콜/패킷 레벨"이었다. VarInt(LEB128), Position 패킹(26/12/26), zlib 압축 프레이밍, Configuration 상태 전이, 오프라인 UUID(MD5)까지 전부 직접 구현하고 **위키 공식 테스트 벡터**로 검증했다.
- 패킷 ID 는 `minecraft-data` 의 1.21.1 정의와 대조해 맞췄다. (예: Play clientbound KeepAlive = `0x26`, Chunk = `0x27`)

## 빌드 · 실행

```bash
cargo build --release

# 1) 패킷 분석 (서버 목록 핑)
./target/release/packet-probe status 127.0.0.1:25566

# 2) 로그인 후 Play 패킷 6초 수집·분류 (킥 없이)
./target/release/packet-probe probe 127.0.0.1:25566 ProbeBot 6

# 3) 설계도대로 건축
./target/release/build-bot 127.0.0.1:25566 BuildBot blueprints/cottage.txt
```

### 로컬 테스트 서버 (오프라인)

바닐라 1.21.1 서버를 `online-mode=false`, `level-type=minecraft:flat`, `gamemode=creative` 로 띄우면 된다.
(build-bot 은 크리에이티브에서 블록을 손에 들어 설치한다. 아래 "로드맵" 참고.)

## 설계도 포맷

사람이 손으로 그리기 쉬운 레이어/팔레트 텍스트. `blueprints/` 예시 참고.

```text
name 작은 오두막
palette W white_concrete
palette G glass
layer 0          # y=0 평면. 아래 각 줄이 z 증가, 글자가 x 증가
WWW
W.W              # '.' 또는 공백 = 공기(건너뜀)
WWW
layer 1
WGW
...
```

지원 블록 이름은 `crates/mc-bot/src/blocks.rs`(생성됨) 참고. 흰/빨강 등 콘크리트, 유리, 양모, 돌 종류 등 44종.

## 구조

```
mc-build-bot/
├─ crates/mc-proto/      프로토콜 기초 (의존성: flate2 뿐)
│  ├─ buf.rs             VarInt/VarLong, 문자열, Position, UUID …
│  └─ frame.rs           길이 프리픽스 + zlib 압축 프레이밍
└─ crates/mc-bot/
   ├─ net.rs             TCP 연결 래퍼
   ├─ proto.rs           핸드셰이크·상태핑·오프라인 로그인(MD5 UUID)·Config
   ├─ ids.rs             1.21.1 패킷 ID (minecraft-data 대조)
   ├─ world.rs           좌표·시야(yaw/pitch)·도달거리
   ├─ schematic.rs       설계도 파서
   ├─ builder.rs         Play 클라이언트: 이동·설치·keepalive 응답
   ├─ blocks.rs          블록→아이템 ID (생성)
   └─ bin/               packet-probe, build-bot
```

## 검증

- `cargo test` — 19개 (VarInt 공식 벡터, Position 왕복, zlib, MD5 RFC 벡터, 설계도 파싱, 도달거리 등)
- 실서버(로컬 1.21.1): `probe` 가 keepalive·텔레포트에 응답하며 **킥 없이** 패킷 수집.
  `build-bot` 이 3×3 오두막(25블록)을 **전부 설치**(흰12·유리4·빨강9), 도달실패 0, 킥 없음 — RCON 개수로 확인.

## 이동과 anti-cheat

- 항상 `on_ground=true` 로, 한 틱당 이동량을 걷기 속도(~0.26블록) 이하로 제한해 조금씩 보낸다 → "moved too quickly"/"flying is not enabled" 로 **킥당하지 않는다.**
- 블록은 "아래 블록의 윗면"을 눌러 설치하므로 받침 없는 공중 설치를 하지 않는다.
- 서버의 Synchronize Player Position(텔레포트)에는 Confirm Teleport 로 즉시 응답한다.

## 로드맵 (다음 단계)

- **생존 모드 + 상자에서 자재 꺼내기**: "주변 상자에 블록을 넣어두면 꺼내서 짓기". 필요한 패킷 ID 는 이미 매핑돼 있다 — Open Screen(`0x33`), Set Container Content(`0x13`), Window Click(`0x0E`), Close(`0x0F`). 1.21 의 새 Slot 컴포넌트 포맷 파싱이 관건.
- **비계 쌓아 올라가기(pillar-up)**: 도달 범위를 넘는 높은 구조물.
- **.litematic / .schem(스펀지 NBT) 불러오기**: 외부 설계도 포맷.
- **ViaProxy 연동**으로 최신 azalea 없이 다른 버전 서버 접속.

## 라이선스

MIT. 패킷 ID 는 PrismarineJS/minecraft-data(1.21.1)와 대조했다.
