package main

import (
	"log"
	"net/http"
	"os"
)

func main() {
	port := os.Args[1]

	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		log.Println("Worker received a request")
		w.Write([]byte("Hello, World!"))
	})

	log.Println("Worker is running on port", port)
	log.Fatal(http.ListenAndServe(":"+port, nil))
}
