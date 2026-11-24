package mobile

import (
	"encoding/json"
	"net"
	"sync"

	"tailscale.com/net/netmon"
)

// AndroidNetworkInterfaces supplies a snapshot from Android's Java networking API.
type AndroidNetworkInterfaces interface {
	Snapshot() string
}

var registerAndroidNetworkInterfaces sync.Once

// RegisterAndroidNetworkInterfaces installs the process-wide interface getter before Engine startup.
func RegisterAndroidNetworkInterfaces(source AndroidNetworkInterfaces) {
	registerAndroidNetworkInterfaces.Do(func() {
		netmon.RegisterInterfaceGetter(func() ([]netmon.Interface, error) {
			var snapshot []struct {
				Name      string   `json:"name"`
				Index     int      `json:"index"`
				MTU       int      `json:"mtu"`
				Up        bool     `json:"up"`
				Loopback  bool     `json:"loopback"`
				Addresses []string `json:"addresses"`
			}
			if err := json.Unmarshal([]byte(source.Snapshot()), &snapshot); err != nil {
				return nil, nil
			}
			interfaces := make([]netmon.Interface, 0, len(snapshot))
			for _, item := range snapshot {
				iface := &net.Interface{Name: item.Name, Index: item.Index, MTU: item.MTU}
				if item.Up {
					iface.Flags |= net.FlagUp
				}
				if item.Loopback {
					iface.Flags |= net.FlagLoopback
				}
				addresses := make([]net.Addr, 0, len(item.Addresses))
				for _, cidr := range item.Addresses {
					ip, subnet, err := net.ParseCIDR(cidr)
					if err == nil {
						subnet.IP = ip
						addresses = append(addresses, subnet)
					}
				}
				interfaces = append(interfaces, netmon.Interface{Interface: iface, AltAddrs: addresses})
			}
			return interfaces, nil
		})
	})
}
