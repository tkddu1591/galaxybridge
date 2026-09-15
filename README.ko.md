# GalaxyBridge

안드로이드의 RNDIS 방식 USB 테더링을 Apple Silicon 맥의 인터넷 연결로 사용하는 독립 드라이버입니다. 삼성 제조사 제한 없이 USB 인터페이스와 프로토콜 구성을 검사합니다. TetherKit·HoRNDIS·libusb에 의존하지 않습니다. Rust로 RNDIS를 구현하고, `nusb`로 macOS IOKit에 접근합니다.

**초기 실험 버전입니다.** 모든 M 시리즈·안드로이드 조합에서 검증된 제품으로 오해하면 안 됩니다. 실제 기기 검증은 Galaxy S25 Ultra에서 수행했습니다. 실제 확인 범위는 [영문 README](README.md)의 검증 표를 참고하세요.

## 1. 명령 하나로 설치

Apple Silicon 맥의 터미널에 아래 **명령 하나를 복사해 실행**합니다.

```sh
/bin/bash -c 'setup=$(/usr/bin/curl --disable -fsS --proto "=https" https://raw.githubusercontent.com/tkddu1591/galaxybridge/main/setup.sh) && /bin/bash -c "$setup"'
```

고정된 릴리스 다운로드 → SHA-256 검증 → 관리자 인증 → **자동 재연결 서비스 설치** 순서로 진행합니다. 설치 후 폰을 연결하고 **USB 테더링**을 켜면 됩니다. 매번 터미널을 켜거나 관리자 비밀번호를 입력할 필요가 없습니다.

Apple Command Line Tools가 없다면 Apple 설치 창이 열립니다. 창에서 설치를 완료하면 계속 진행합니다. 대기 시간이 초과되면 도구 설치를 마친 뒤 같은 명령을 다시 실행하면 됩니다. 이 도구는 설치 파일 검증에만 필요하며 Rust·Homebrew는 필요 없습니다.

다운로드가 성공한 경우에만 설치 스크립트를 실행하며, 관리자 권한으로 파일을 다운로드하지 않습니다. GitHub 저장소와 HTTPS 전달 경로를 신뢰하는 방식입니다. 체크섬·임시 서명만으로 별도의 배포자 신원이 보증되지는 않습니다. 실행 전에 [setup.sh](https://github.com/tkddu1591/galaxybridge/blob/main/setup.sh)를 읽을 수 있습니다.

기존 설치가 있으면 보존하고 중단합니다. 이 명령은 최초 설치용이며, 교체하려면 기존 버전의 제거 명령을 먼저 실행합니다.

<details>
<summary>수동 연결·대상 폰 지정·오프라인 설치</summary>

[릴리스](https://github.com/tkddu1591/galaxybridge/releases)의 압축 파일과 체크섬을 받아 검증하고, 본인 소유 폴더에 압축을 풉니다. `./install.sh`는 수동 연결용으로, `./install.sh --auto`는 자동 재연결용으로 설치합니다. 내려받은 `setup.sh --manual`도 백그라운드 서비스 없이 설치합니다.

수동 연결은 폰에서 USB 테더링을 켠 뒤 실행합니다.

```sh
galaxybridge devices
sudo galaxybridge connect
```

터미널을 유지하고 `Ctrl+C`로 종료합니다. 특정 폰은 오프라인 설치 시 `./install.sh --auto --serial YOUR_PHONE_SERIAL`로 선택할 수 있습니다.

</details>

## 2. 폰 설정도 USB 연결 시 자동으로

폰에도 **설정 → 개발자 옵션 → 기본 USB 구성 → USB 테더링** 항목이 있다면 선택합니다. USB 디버깅은 필요 없습니다. 이 항목이 없거나 적용되지 않는 기기는 폰에서 USB 테더링을 직접 켜야 합니다.

폰 자동 활성화 설정과 맥의 자동 연결 서비스는 별개입니다. 둘 다 적용되면 연결 감지·드라이버 시작·DHCP·IPv4 경로 설정을 자동으로 처리합니다. 최초 설치 뒤 매번 관리자 비밀번호를 요구하지 않습니다. 폰 정책에 따라 잠금 해제가 필요할 수 있습니다.

USB를 빼면 기존에 연결돼 있던 Wi-Fi의 현재 경로를 복구합니다. 0.2.0의 M3 Pro·macOS 26.5.1·Galaxy S25 Ultra 시험 두 번에서 Wi-Fi를 끄고 켜지 않고도 USB 제거 감지 후 약 0.38–0.60초에 Wi-Fi 경로, 약 0.72–0.90초에 새 HTTPS 요청이 복구됐습니다. 두 차례 3분 연결 유지와 10MiB 송수신도 통과했습니다. 이 수치는 해당 기기·환경의 측정값입니다. [실기기 검증 기록](docs/validation-0.2.0.md)을 확인하세요.

여러 폰이 잡히면 임의로 연결하지 않습니다. 필요한 경우 `--vendor`, `--product`, `--serial`로 대상을 좁힙니다. 시리얼은 장치 선택 보조 수단이며 인증 수단은 아닙니다.

NCM·ECM 방식은 별도 프로토콜이므로 GalaxyBridge가 해당 인터페이스를 가로채지 않습니다. 이런 기기는 macOS의 시스템 설정 → 네트워크에서 기본 USB 네트워크 서비스가 잡히는지 확인하세요. 제조사 이름만으로 프로토콜이나 호환성을 판단할 수 없습니다.

## 3. 보안

**보안 문제가 전혀 없다고 보장하지 않습니다.** 관리자 프로세스와 비관리자 USB 프로세스를 분리하고, 입력 크기·오프셋·상태를 검사합니다. USB 작업자는 BPF 파일을 받지 않고 제한된 데이터그램 소켓만 받습니다.

그러나 관리자 서비스 자체, macOS의 USB/네트워크 스택, 외부 Rust 라이브러리에는 여전히 위험이 있습니다. USB 작업자는 전용 로그인 불가 계정과 App Sandbox 안에서 실행합니다. 직접적인 일반 파일·네트워크 접근은 제한하지만, 허용된 USB·내부 통신 경로는 여전히 사용할 수 있습니다. 연결된 폰은 DHCP·DNS·네트워크 데이터를 제공하는 신뢰 대상입니다.

SIP 해제나 맥 보안 수준 변경은 필요 없습니다. 텔레메트리·패킷 내용 기록·백그라운드 코드 다운로드는 하지 않습니다. 사용자가 설치 명령을 실행할 때만 릴리스를 다운로드합니다. 체크섬과 임시 코드 서명은 파일 손상을 확인하는 장치이며 배포자 신원을 보증하지 않습니다. [자세한 보안 모델](SECURITY.md)을 확인하세요.

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
