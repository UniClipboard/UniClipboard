import { useState, useEffect } from 'react';
import NetInfo, { NetInfoState } from '@react-native-community/netinfo';

export type NetworkType = 'wifi' | 'cellular' | 'other' | 'none';

export function useNetworkState() {
  const [isConnected, setIsConnected] = useState(true);
  const [networkType, setNetworkType] = useState<NetworkType>('other');

  useEffect(() => {
    const unsubscribe = NetInfo.addEventListener((state: NetInfoState) => {
      setIsConnected(state.isConnected ?? false);
      if (!state.isConnected) {
        setNetworkType('none');
      } else if (state.type === 'wifi') {
        setNetworkType('wifi');
      } else if (state.type === 'cellular') {
        setNetworkType('cellular');
      } else {
        setNetworkType('other');
      }
    });
    return () => unsubscribe();
  }, []);

  return { isConnected, networkType, isWifi: networkType === 'wifi' };
}
