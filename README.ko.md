# GalaxyBridge

안드로이드의 RNDIS 방식 USB 테더링을 Apple Silicon 맥의 인터넷 연결로 사용하는 독립 드라이버입니다. 삼성 제조사 제한 없이 USB 인터페이스와 프로토콜 구성을 검사합니다. TetherKit·HoRNDIS·libusb에 의존하지 않습니다. Rust로 RNDIS를 구현하고, `nusb`로 macOS IOKit에 접근합니다.

**초기 실험 버전입니다.** 모든 M 시리즈·안드로이드 조합에서 검증된 제품으로 오해하면 안 됩니다. 실제 기기 검증은 Galaxy S25 Ultra에서 수행했습니다. 실제 확인 범위는 [영문 README](README.md)의 검증 표를 참고하세요.

## 1. 맥에서 안드로이드 USB 테더링

설치 프로그램은 Apple의 `lipo`와 `otool`로 바이너리를 검증합니다. 이 도구가 없다면 **Apple Command Line Tools를 한 번 설치**해야 합니다: `xcode-select --install`. 설정된 전체 Xcode가 있어도 됩니다. 설치 과정의 검증에만 필요한 도구이며, 설치 후 GalaxyBridge 실행에는 Command Line Tools·Rust·Homebrew가 필요하지 않습니다.

[릴리스](https://github.com/tkddu1591/galaxybridge/releases)의 arm64 압축 파일과 체크섬을 받아 검증하고, 압축을 푼 폴더에서 설치합니다.

```sh
./install.sh
```

폰에서 모바일 데이터와 **USB 테더링**을 켜고 데이터 케이블로 연결한 뒤:

```sh
galaxybridge devices
sudo galaxybridge connect
```

실행 터미널을 유지합니다. 종료는 `Ctrl+C`입니다. 브라우저 프록시가 아니라 macOS 네트워크 인터페이스를 만드는 방식입니다.

## 2. USB만 꽂으면 연결 — 선택 사항

자동 재연결 서비스를 원하면 최초 설치에 `--auto`를 붙입니다.

```sh
./install.sh --auto
```

폰에도 **설정 → 개발자 옵션 → 기본 USB 구성 → USB 테더링** 항목이 있다면 선택합니다. USB 디버깅은 필요 없습니다. 이 항목이 없거나 적용되지 않는 기기는 폰에서 USB 테더링을 직접 켜야 합니다.

폰 자동 활성화 설정과 맥의 자동 연결 서비스는 별개입니다. 둘 다 적용되면 연결 감지·드라이버 시작·DHCP·IPv4 경로 설정을 자동으로 처리합니다. 최초 설치 뒤 매번 관리자 비밀번호를 요구하지 않습니다. 폰 정책에 따라 잠금 해제가 필요할 수 있습니다.

USB를 빼면 기존에 연결돼 있던 Wi-Fi의 현재 경로를 복구합니다. 0.1.0 버전의 M3 Pro·Galaxy S25 Ultra 시험에서 Wi-Fi를 끄고 켜지 않은 채 분리한 시험에서는 약 0.79초에 Wi-Fi 경로가 돌아왔고, 약 1.09초에 새 HTTPS 요청이 성공했습니다. 이 시간은 해당 버전·환경의 측정값이며, 새 샌드박스 작업자는 별도 실기 검증이 필요합니다.

여러 폰이 잡히면 임의로 연결하지 않습니다. 필요한 경우 `--vendor`, `--product`, `--serial`로 대상을 좁힙니다. 시리얼은 장치 선택 보조 수단이며 인증 수단은 아닙니다.

NCM·ECM 방식은 별도 프로토콜이므로 GalaxyBridge가 해당 인터페이스를 가로채지 않습니다. 이런 기기는 macOS의 시스템 설정 → 네트워크에서 기본 USB 네트워크 서비스가 잡히는지 확인하세요. 제조사 이름만으로 프로토콜이나 호환성을 판단할 수 없습니다.

## 3. 보안

**보안 문제가 전혀 없다고 보장하지 않습니다.** 관리자 프로세스와 비관리자 USB 프로세스를 분리하고, 입력 크기·오프셋·상태를 검사합니다. USB 작업자는 BPF 파일을 받지 않고 제한된 데이터그램 소켓만 받습니다.

그러나 관리자 서비스 자체, macOS의 USB/네트워크 스택, 외부 Rust 라이브러리에는 여전히 위험이 있습니다. USB 작업자는 전용 로그인 불가 계정과 App Sandbox 안에서 실행합니다. 직접적인 일반 파일·네트워크 접근은 제한하지만, 허용된 USB·내부 통신 경로는 여전히 사용할 수 있습니다. 연결된 폰은 DHCP·DNS·네트워크 데이터를 제공하는 신뢰 대상입니다.

SIP 해제나 맥 보안 수준 변경은 필요 없습니다. 텔레메트리·패킷 내용 기록·자동 코드 다운로드는 하지 않습니다. 체크섬과 임시 코드 서명은 파일 손상을 확인하는 장치이며 배포자 신원을 보증하지 않습니다. [자세한 보안 모델](SECURITY.md)을 확인하세요.

## 중지·제거

```sh
# 자동 서비스 일시 중지
sudo launchctl bootout system/io.galaxybridge

# 다시 실행
sudo launchctl bootstrap system /Library/LaunchDaemons/io.galaxybridge.plist

# 제거
sudo /Library/PrivilegedHelperTools/io.galaxybridge/uninstall.sh
```

IPv4 경로를 설정하며, VPN·IPv6·DNS까지 모든 통신이 폰으로만 흐른다는 보장은 하지 않습니다. 비공개 `feth` API를 사용하므로 macOS 업데이트 후 재검증이 필요합니다.
