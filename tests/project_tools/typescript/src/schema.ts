import { z } from "zod";

export const EmployeeSchema = z.object({
    name: z.string(),
    department: z.string(),
    salary: z.number().min(30000)
});