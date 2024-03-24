package main

import (
	"log"
	"net/http"
	"os"
)

func main() {
	port := os.Args[1]

	http.HandleFunc("GET /worker/{id}", func(w http.ResponseWriter, r *http.Request) {
		id := r.PathValue("id")
		log.Println("Worker received a request", id)
		w.Write([]byte("Hello, World!"))
	})

	log.Println("Worker is running on port", port)
	log.Fatal(http.ListenAndServe(":"+port, nil))
}
