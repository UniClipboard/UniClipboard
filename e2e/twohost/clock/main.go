// Command clock provides timestamps over one persistent SSH stream.
// It does not participate in unic content submission or reception.
package main

import (
	"bufio"
	"fmt"
	"os"
	"time"
)

func main() {
	fmt.Println("ready")
	scanner := bufio.NewScanner(os.Stdin)
	for scanner.Scan() {
		fmt.Println(time.Now().UnixNano())
	}
	if scanner.Err() != nil {
		os.Exit(1)
	}
}
