package parser

import "github.com/tidwall/gjson"

func GetNames(jsonStr string) []string {
	result := gjson.Get(jsonStr, "users.#.name")
	var names []string
	for _, name := range result.Array() {
		names = append(names, name.String())
	}
	return names
}