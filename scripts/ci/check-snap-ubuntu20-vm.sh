#!/usr/bin/env bash
# Disposable diagnostic only: boot the real Focal kernel/userspace and run the existing Snap harness.
set -euo pipefail
[[ ${GITHUB_ACTIONS:-} == true && ${RUNNER_ENVIRONMENT:-} == github-hosted ]] || exit 2
package=$(realpath "${1:?Snap artifact}")
harness=$(realpath "${2:?harness from the artifact source commit}")
vm_dir="${RUNNER_TEMP:?}/snap-ubuntu20-vm"
mkdir -p "$vm_dir/shared/evidence"
cp "$package" "$vm_dir/shared/package.snap"
cp "$harness" "$vm_dir/shared/check-snap.sh"
chmod 777 "$vm_dir/shared" "$vm_dir/shared/evidence"
sudo apt-get update -qq
sudo apt-get install -y --no-install-recommends qemu-system-x86 qemu-utils cloud-image-utils
curl -fL --retry 3 --max-time 300 https://cloud-images.ubuntu.com/focal/20250624/focal-server-cloudimg-amd64.img -o "$vm_dir/focal.img"
printf '18f2977d77dfea1b74aee14533bd21c34f789139e949c57023b7364894b7e5e9  %s\n' "$vm_dir/focal.img" | sha256sum -c -
qemu-img create -f qcow2 -F qcow2 -b "$vm_dir/focal.img" "$vm_dir/guest.qcow2" 16G
cat >"$vm_dir/user-data" <<'CLOUD'
#cloud-config
ssh_deletekeys: true
ssh_genkeytypes: []
users:
  - name: uc
    lock_passwd: true
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
write_files:
  - path: /root/uc-verify.sh
    permissions: '0755'
    content: |
      #!/usr/bin/env bash
      set -euo pipefail
      mkdir -p /mnt/uc
      mount -t 9p -o trans=virtio,version=9p2000.L uc /mnt/uc
      exec >/mnt/uc/guest.log 2>&1
      finish() { rc=$?; printf '%s\n' "$rc" >/mnt/uc/result-code; sync; poweroff; }
      trap finish EXIT
      { cat /etc/os-release; uname -a; snap version; } >/mnt/uc/host.txt
      . /etc/os-release
      [[ "$VERSION_ID" == 20.04 ]]
      systemctl start snapd.socket
      timeout 180s snap wait system seed.loaded
      cp /mnt/uc/package.snap /home/uc/package.snap
      chown uc:uc /home/uc/package.snap
      timeout --kill-after=15s 900s runuser -u uc -- env HOME=/home/uc GITHUB_ACTIONS=true bash /mnt/uc/check-snap.sh /home/uc/package.snap /mnt/uc/evidence
runcmd:
  - [/bin/bash, /root/uc-verify.sh]
CLOUD
printf 'instance-id: uc-focal-snap\nlocal-hostname: uc-focal-snap\n' >"$vm_dir/meta-data"
cloud-localds "$vm_dir/seed.img" "$vm_dir/user-data" "$vm_dir/meta-data"
accelerator=(-accel tcg,thread=multi -cpu max)
if [[ -e /dev/kvm ]]; then
  sudo chown "$(id -u):$(id -g)" /dev/kvm
  accelerator=(-accel kvm -cpu host)
fi
printf 'accelerator: %s\n' "${accelerator[*]}" >"$vm_dir/accelerator.txt"
set +e
timeout --kill-after=15s 1500s qemu-system-x86_64 -machine q35 "${accelerator[@]}" -smp 2 -m 4096 \
  -display none -serial stdio -monitor none -no-reboot \
  -drive "file=$vm_dir/guest.qcow2,if=virtio,format=qcow2" \
  -drive "file=$vm_dir/seed.img,if=virtio,format=raw,readonly=on" \
  -nic user,model=virtio-net-pci \
  -virtfs "local,path=$vm_dir/shared,mount_tag=uc,security_model=mapped-xattr" \
  >"$vm_dir/console.log" 2>&1
vm_rc=$?
set -e
cat "$vm_dir/shared/host.txt" "$vm_dir/accelerator.txt" 2>/dev/null || true
[[ $vm_rc == 0 && -f "$vm_dir/shared/result-code" ]] || { tail -80 "$vm_dir/console.log"; exit 1; }
[[ $(cat "$vm_dir/shared/result-code") == 0 ]] || { tail -80 "$vm_dir/shared/guest.log"; exit 1; }
[[ -f "$vm_dir/shared/evidence/result.txt" ]]
cat "$vm_dir/shared/evidence/result.txt"
