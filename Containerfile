FROM quay.io/archlinux/archlinux:latest AS builder

RUN <<EOR

set -euxo pipefail

# Install build dependencies available in the official repos
pacman -Sy
pacman --noconfirm -S cargo git sudo base-devel ostree

# Create an unprivileged builder user and let it use sudo
useradd -m builder
echo "builder ALL=(ALL:ALL) NOPASSWD: ALL" > /etc/sudoers.d/99-allow-builder

# Install paru as an AUR build helper
cd ~builder
sudo -u builder git clone https://aur.archlinux.org/paru.git
cd paru
sudo -u builder makepkg
pacman --noconfirm -U paru*.pkg.tar.zst

# Install pacman-static in order to be able to statically link libalpm into oci-chunker
sudo -u builder paru --noconfirm --skipreview -Sa pacman-static

mkdir /build
EOR

COPY Cargo.* /build/
COPY src /build/src/

RUN <<EOR

set -euxo pipefail

# Build oci-chunker
cd /build
ls
cargo build --features archlinux --release

EOR

FROM quay.io/archlinux/archlinux:latest

COPY --from=builder /build/target/release/oci-chunker /usr/local/bin
RUN pacman -Sy && pacman --noconfirm -S ostree podman skopeo
