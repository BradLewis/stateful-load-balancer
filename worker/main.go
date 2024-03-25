package main

import (
	"encoding/json"
	"log"
	"net/http"
	"os"
	"strconv"
)

func main() {
	port := os.Args[1]

	workers := []int{}

	http.HandleFunc("GET /worker/{id}", func(w http.ResponseWriter, r *http.Request) {
		id := r.PathValue("id")
		log.Println("Worker received a request", id)
		workerId, _ := strconv.Atoi(id)
		workers = append(workers, workerId)
		w.Write([]byte("Hello, World!"))
	})

	http.Handle("GET /health", http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		jsonData, err := json.Marshal(workers)
		if err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		w.Write(jsonData)
	}))

	log.Println("Worker is running on port", port)
	log.Fatal(http.ListenAndServe(":"+port, nil))
}
